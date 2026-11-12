"""Black-box tests of `task-pending-page` and `task-pending-status`.

Roots are built with hf-fixture-seed and read with the built debug harflow-ctl. The oracle is
Python's own hashlib, base64, json and sqlite3. The reference is HarflowMCP commit 0ac7e3c
(`task_pending_reads.py`): the namespace revision is its SHA-256 over the sorted receipt
filenames, each followed by a NUL, and the cursor is `urlsafe_b64encode` of compact JSON with
sorted keys.
"""

from pending_support import *  # noqa: F401,F403
from pending_support import PendingCase


class PageTests(PendingCase):
    def test_an_empty_namespace_is_complete_and_hashes_nothing(self):
        page = self.page()
        self.assertEqual(set(page), PAGE_KEYS)
        self.assertEqual(page["namespace_revision"], hashlib.sha256(b"").hexdigest())
        self.assertEqual((page["items"], page["coverage"], page["examined_count"]), ([], "complete", 0))
        self.assertEqual((page["has_more"], page["next_cursor"], page["bound"]), (False, None, None))
        self.assertIs(page["namespace_complete"], True)
        self.assertEqual(page["scope"], SCOPE)

    def test_launches_are_listed_in_filename_order_with_their_revision(self):
        for name in (C, A, B):
            self.add_launch(name)
        page = self.page()
        self.assertEqual(self.names(page), [A, B, C])
        self.assertEqual(page["namespace_revision"], revision(*(n + SUFFIX for n in (A, B, C))))
        self.assertEqual(page["coverage"], "complete")
        self.assertRegex(page["observed_at"], r"^\d{4}-\d\d-\d\dT[\d:.]+\+00:00$")
        item = page["items"][0]
        self.assertEqual(set(item), {"task_id", "disposition", "launch", "cancel_support"})
        self.assertEqual(set(item["launch"]), LAUNCH_KEYS)
        self.assertEqual((item["disposition"], item["cancel_support"]), ("pending", "unsupported"))

    def test_an_invalid_name_is_counted_hashed_and_never_echoed(self):
        self.add_launch(A)
        self.seed("add_invalid_launch_name", {"name": "bad name"})
        page = self.page()
        self.assertEqual(page["invalid_name_count"], 1)
        self.assertEqual(page["namespace_revision"], revision(A + SUFFIX, "bad name" + SUFFIX))
        self.assertEqual(page["coverage"], "unknown")
        self.assertNotIn("bad name", json.dumps(page["items"]))
        self.assertEqual(self.names(page), [A])

    def test_ids_that_are_prefixes_of_each_other_are_both_listed_at_limit_one(self):
        self.add_launch("ab")
        self.add_launch("ab-c")
        pages = self.walk("--limit", "1")
        self.assertEqual([self.names(page) for page in pages], [["ab-c"], ["ab"]])
        self.assertEqual(pages[0]["namespace_revision"], revision("ab" + SUFFIX, "ab-c" + SUFFIX))

    def test_a_cursor_has_the_reference_shape_and_bytes(self):
        for name in (A, B, C):
            self.add_launch(name)
        first = self.page("--limit", "2")
        cursor = first["next_cursor"]
        value = decode(cursor)
        self.assertEqual(set(value), {"v", "scope", "root", "identity", "revision", "last", "limit", "uncertain"})
        self.assertEqual((value["v"], value["scope"], value["last"], value["limit"]), (1, SCOPE, B, 2))
        self.assertEqual(value["root"], hashlib.sha256(os.fsencode(self.state)).hexdigest())
        self.assertEqual(value["revision"], first["namespace_revision"])
        self.assertIs(value["uncertain"], False)
        self.assertTrue(len(value["identity"]) == 4 and all(type(i) is int for i in value["identity"]))
        expected = base64.urlsafe_b64encode(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())
        self.assertEqual(cursor.encode(), expected)
        second = self.page("--limit", "2", "--cursor", cursor)
        self.assertEqual((self.names(second), second["has_more"], second["coverage"]), ([C], False, "complete"))
        self.assertEqual(first["coverage"], "unknown")

    def test_a_cursor_written_by_the_reference_is_accepted(self):
        for name in (A, B):
            self.add_launch(name)
        value = decode(self.page("--limit", "1")["next_cursor"])
        value["identity"] = list(value["identity"])
        spelled = base64.urlsafe_b64encode(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).decode()
        self.assertEqual(self.names(self.page("--limit", "1", "--cursor", spelled)), [B])

    def test_the_cursor_survives_a_change_of_content_but_not_a_new_name(self):
        for name in (A, B):
            self.add_launch(name)
        cursor = self.page("--limit", "1")["next_cursor"]
        self.add_launch(B, "granted", title="Changed")
        resumed = self.page("--limit", "1", "--cursor", cursor)
        self.assertEqual(resumed["items"][0]["launch"]["state"], "granted")
        self.add_launch(C)
        error = self.refused("task-pending-page", "--limit", "1", "--cursor", cursor,
                             code="pending_cursor_expired", check="namespace_changed")
        self.assertIn("(namespace_changed)", error["message"])

    def test_a_cursor_for_another_limit_or_with_a_vanished_position_is_refused(self):
        for name in (A, B):
            self.add_launch(name)
        cursor = self.page("--limit", "1")["next_cursor"]
        self.refused("task-pending-page", "--limit", "2", "--cursor", cursor,
                     code="pending_cursor_invalid", check="cursor_scope")
        self.refused("task-pending-page", "--cursor", cursor,
                     code="pending_cursor_invalid", check="cursor_scope")
        self.refused("task-pending-page", "--limit", "1", "--cursor", "x",
                     code="pending_cursor_invalid", check="cursor_shape")

    def test_limits_outside_the_range_are_refused(self):
        self.add_launch(A)
        for limit in ("0", "101", "-1"):
            self.refused("task-pending-page", "--limit", limit, code="invalid_pending_limit", check="limit_1_to_100")
        self.assertEqual(len(self.page("--limit", "100")["items"]), 1)

    def test_a_record_with_the_same_id_filters_the_launch_but_counts_it(self):
        self.add(A)
        self.add_launch(A)
        self.add_launch(B)
        page = self.page()
        self.assertEqual((self.names(page), page["examined_count"], page["coverage"]), ([B], 2, "complete"))

    def test_spawned_and_settled_receipts_are_filtered(self):
        self.add_launch(A, "spawned")
        self.add_launch(B, "settled")
        self.add_launch(C)
        page = self.page()
        self.assertEqual((self.names(page), page["examined_count"], page["coverage"]), ([C], 3, "complete"))


class DisplayTests(PendingCase):
    def test_cancel_support_needs_resources_or_a_worktree_and_a_started_launch(self):
        cases = {
            A: ("reserved", {}, "unsupported"),
            B: ("granted", {"resources": {"names": ["gpu"]}}, "supported"),
            C: ("claimed", {"worktree": {"path": "/w"}}, "supported"),
            "d" * 32: ("not_started", {"resources": {"names": ["gpu"]}}, "unsupported"),
        }
        for name, (state, fields, _) in cases.items():
            self.add_launch(name, state, **fields)
        page = self.page()
        for item in page["items"]:
            self.assertEqual(item["cancel_support"], cases[item["task_id"]][2], item["task_id"])

    def test_lineage_keeps_only_valid_ids_and_empty_reasons_stay(self):
        self.add_launch(A, "not_started", not_started_reason="", lineage={
            "spawned_by": B, "root_task_id": "not valid!", "parent_task_id": C, "chain_root_task_id": None})
        launch_view = self.page()["items"][0]["launch"]
        self.assertEqual(launch_view["lineage"], {"spawned_by": B, "parent_task_id": C})
        self.assertEqual(launch_view["not_started_reason"], "")

    def test_text_is_cut_on_a_character_boundary_and_reported(self):
        self.add_launch(A, title="é" * 600, repo_path="p" * 4100)
        view = self.page()["items"][0]["launch"]
        self.assertEqual(view["title"], "é" * 512)
        self.assertEqual(len(view["repo_path"]), 4096)
        self.assertIs(view["metadata_truncated"], True)

    def test_resource_objects_without_names_or_with_bad_names_are_refused(self):
        self.add_launch(A, resources={})
        self.add_launch(B, resources={"names": []})
        self.add_launch(C, worktree={})
        by_id = {item["task_id"]: item for item in self.page()["items"]}
        self.assertEqual((by_id[A]["code"], by_id[A]["check"]), ("field_shape", "field_shape"))
        self.assertEqual((by_id[B]["code"], by_id[B]["check"]), ("field_shape", "resource_names"))
        self.assertEqual((by_id[C]["code"], by_id[C]["check"]), ("field_shape", "field_shape"))
        self.assertEqual(self.page()["coverage"], "unknown")

    def test_invalid_utf8_in_a_display_column_is_refused_and_the_walk_is_unknown(self):
        self.add_launch(A, resources={"names": ["gpu"]})
        self.add_launch(B)
        self.sql("UPDATE launch_attempts SET title=CAST(x'ff80' AS TEXT) WHERE name=?", (A,))
        page = self.page()
        refused, listed = page["items"]
        self.assertEqual(refused, {"task_id": A, "disposition": "refused", "code": "field_shape",
                                   "check": "display_text_encoding"})
        self.assertEqual(listed["task_id"], B)
        self.assertEqual(page["coverage"], "unknown")
        again = self.status(A)
        self.assertEqual((again["disposition"], again["check"]), ("refused", "display_text_encoding"))
        self.assertEqual(self.bad("status", A)["code"], "task_pending_launch")

    def test_receipts_refused_at_qualification_carry_their_check(self):
        for check in ("format", "version", "root_mismatch", "task_id_mismatch", "generation", "oversize", "field_shape"):
            with self.subTest(check=check):
                self.seed("remove_root")
                self.init()
                self.add_launch(A, refusal={"check": check})
                self.assertEqual(self.page()["items"], [
                    {"task_id": A, "disposition": "refused", "code": check, "check": check}])
                self.assertEqual(self.page()["coverage"], "unknown")


class StatusTests(PendingCase):
    def test_each_disposition(self):
        self.add_launch(A, "granted", resources={"names": ["gpu"]})
        self.add_launch(B, "spawned")
        self.add_launch(C, refusal={"check": "format"})
        self.add("dddddddd-dddd-4ddd-8ddd-dddddddddddd")
        self.add_launch("dddddddd-dddd-4ddd-8ddd-dddddddddddd")
        pending = self.status(A)
        self.assertEqual((pending["disposition"], pending["cancel_support"]), ("pending", "supported"))
        self.assertEqual(set(pending), {"scope", "observed_at", "task_id", "disposition", "launch", "cancel_support"})
        self.assertEqual(self.status(B)["disposition"], "unknown")
        self.assertEqual((self.status(B)["code"], self.status(B)["check"]),
                         ("pending_record_inconsistent", "postrecord_source_without_record"))
        self.assertEqual((self.status(C)["code"], self.status(C)["check"]), ("format", "format"))
        self.assertEqual(self.status("dddddddd-dddd-4ddd-8ddd-dddddddddddd")["disposition"], "recorded")
        self.assertEqual(self.status(seeds.E)["disposition"], "not_found")

    def test_a_bare_record_is_recorded(self):
        self.add(A)
        self.assertEqual(self.status(A)["disposition"], "recorded")

    def test_only_the_exact_full_id_is_looked_up(self):
        self.add_launch(A)
        for prefix in (A[:8], A[:35]):
            self.assertEqual(self.status(prefix)["disposition"], "not_found", prefix)
        self.refused("task-pending-status", "--task-id", "bad id!", code="invalid_task_id", check="exact_task_id_required")
        self.refused("task-pending-status", "--task-id", "", code="invalid_task_id", check="exact_task_id_required")

    def test_the_status_is_only_what_the_page_shows_for_that_launch(self):
        self.add_launch(A, resources={"names": ["gpu"]}, lineage={"spawned_by": B})
        self.assertEqual(self.status(A)["launch"], self.page()["items"][0]["launch"])

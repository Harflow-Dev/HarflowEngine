"""Black-box tests of the artifact manifest: paging and cursors, the 100 item and byte bounds,
unreadable manifests, ancestry, and the `artifact_summary` of the status documents."""

import base64
import json
import os
import unittest

import seed_support as seeds
from history_support import ArtifactCase, HistoryCase, item
from seed_support import A, B

PAGE_KEYS = {"task_id", "items", "next_cursor", "has_more", "state", "count"}
ENTRY_KEYS = {
    "artifact_id", "task_id", "path", "kind", "note", "registered_at", "provenance",
    "registered_observation", "availability",
}
STALE = "Invalid or stale artifact cursor"


def cursor_of(value):
    return base64.urlsafe_b64encode(json.dumps(value).encode()).decode()


def ident(number):
    return "%08x-0000-4000-8000-000000000000" % number


class PageTests(ArtifactCase):
    def many(self, count, **options):
        self.register(*(item(f"a{n:03d}", f"f{n:03d}.txt", **options) for n in range(count)))

    def test_an_empty_manifest_is_available_with_no_items(self):
        self.register()
        result = self.listed()
        self.assertEqual(set(result), PAGE_KEYS)
        self.assertEqual((result["items"], result["count"], result["state"], result["has_more"], result["next_cursor"]),
                         ([], 0, "available", False, None))

    def test_a_task_that_never_registered_anything_is_available_and_empty(self):
        result = self.listed()
        self.assertEqual((result["items"], result["count"], result["state"]), ([], 0, "available"))

    def test_an_entry_has_exactly_the_documented_keys_in_registration_order(self):
        self.register(item("zz", "z.txt", kind="log", note="last?"), item("aa", "a.txt", note="first?"))
        result = self.listed()
        self.assertEqual([entry["artifact_id"] for entry in result["items"]], ["zz", "aa"])
        for entry in result["items"]:
            self.assertEqual(set(entry), ENTRY_KEYS)
            self.assertEqual(set(entry["registered_observation"]), {"device", "inode", "size", "mtime_ns"})
        self.assertEqual((result["items"][0]["kind"], result["items"][0]["note"]), ("log", "last?"))

    def test_a_prefix_is_echoed(self):
        self.register()
        self.assertEqual(self.listed(task_id=A[:8])["resolved_from"], {"input": A[:8], "task_id": A})

    def test_the_default_page_is_twenty_and_the_limit_is_honoured(self):
        self.many(30)
        self.assertEqual(len(self.listed()["items"]), 20)
        for limit in (1, 7, 30, 50):
            self.assertEqual(len(self.listed("--limit", str(limit))["items"]), min(limit, 30))

    def test_a_walk_serves_each_item_once_and_ends_with_no_cursor(self):
        self.many(100)
        seen, cursor, pages = [], None, 0
        while True:
            arguments = ["--limit", "50"] + (["--cursor", cursor] if cursor else [])
            result = self.listed(*arguments)
            self.assertEqual(result["count"], 100)
            seen += [entry["artifact_id"] for entry in result["items"]]
            pages += 1
            cursor = result["next_cursor"]
            self.assertIs(result["has_more"], cursor is not None)
            if cursor is None:
                break
        self.assertEqual(seen, [f"a{n:03d}" for n in range(100)])
        self.assertEqual(pages, 2)

    def test_the_cursor_has_the_documented_form(self):
        self.many(30)
        cursor = self.listed()["next_cursor"]
        self.assertLessEqual(len(cursor), 512)
        task, revision, offset = json.loads(base64.urlsafe_b64decode(cursor + "=" * (-len(cursor) % 4)))
        self.assertEqual((task, offset), (A, 20))
        self.assertRegex(revision, r"^[0-9a-f]{32}$")

    def test_more_than_one_hundred_items_make_the_manifest_unreadable(self):
        self.many(101)
        result = self.listed()
        self.assertEqual((result["state"], result["items"], result["count"], result["has_more"], result["next_cursor"]),
                         ("unavailable", [], 0, False, None))

    def test_exactly_one_hundred_items_are_readable(self):
        self.many(100)
        result = self.listed("--limit", "50")
        self.assertEqual((result["state"], result["count"], len(result["items"])), ("available", 100, 50))

    def test_a_manifest_whose_header_is_invalid_is_unavailable_and_empty(self):
        self.register(item("x1", "a.txt"), header_valid=False)
        result = self.listed()
        self.assertEqual((result["state"], result["items"], result["count"]), ("unavailable", [], 0))

    def test_the_page_byte_bound_cuts_a_page_short_and_the_walk_still_serves_everything(self):
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt", note="é" * 1000) for n in range(50)))
        cursor, seen, sizes, firsts = None, [], [], []
        while True:
            arguments = ["--limit", "50"] + (["--cursor", cursor] if cursor else [])
            result = self.listed(*arguments)
            sizes.append(len(json.dumps(result)))
            firsts.append(len(json.dumps(result["items"][0])) + 2)
            seen += [entry["artifact_id"] for entry in result["items"]]
            cursor = result["next_cursor"]
            if cursor is None:
                break
        self.assertEqual(seen, [f"a{n:02d}" for n in range(50)])
        self.assertGreater(len(sizes), 1)
        for size in sizes:
            self.assertLessEqual(size, 63_000 + 400)
        for size, nxt in zip(sizes, firsts[1:]):
            self.assertGreater(size + nxt, 63_000, "the page is the largest that fits")


class CursorTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt") for n in range(30)))
        self.cursor = self.listed()["next_cursor"]
        self.task, self.revision, _ = json.loads(base64.urlsafe_b64decode(self.cursor + "=" * (-len(self.cursor) % 4)))

    def stale(self, cursor, task_id=A):
        error = self.bad("task-artifacts", task_id, "--cursor", cursor, code="artifact_error")
        self.assertEqual(error["message"], STALE)

    def test_a_valid_cursor_continues_the_manifest(self):
        result = self.listed("--cursor", self.cursor)
        self.assertEqual(result["items"][0]["artifact_id"], "a20")

    def test_the_offset_may_equal_the_count_and_serves_an_empty_page(self):
        result = self.listed("--cursor", cursor_of([A, self.revision, 30]))
        self.assertEqual((result["items"], result["has_more"], result["next_cursor"]), ([], False, None))

    def test_a_cursor_out_of_range_malformed_or_of_another_shape_is_stale(self):
        for label, value in (
            ("past the end", cursor_of([A, self.revision, 31])), ("negative", cursor_of([A, self.revision, -1])),
            ("text offset", cursor_of([A, self.revision, "5"])), ("float offset", cursor_of([A, self.revision, 5.5])),
            ("bool offset", cursor_of([A, self.revision, True])), ("two items", cursor_of([A, self.revision])),
            ("four items", cursor_of([A, self.revision, 5, 6])), ("object", cursor_of({"a": 1})),
            ("number", cursor_of(5)), ("text", "garbage"), ("empty", ""), ("not json", base64.urlsafe_b64encode(b"x").decode()),
            ("revision", cursor_of([A, "0" * 32, 5])), ("task", cursor_of([B, self.revision, 5])),
            ("long", cursor_of([A, self.revision, 5, "p" * 600])),
        ):
            with self.subTest(cursor=label):
                self.stale(value)

    def test_a_cursor_of_five_hundred_and_twelve_characters_is_the_longest_accepted(self):
        body = json.dumps([A, self.revision, 5])
        fits = base64.urlsafe_b64encode((body + " " * (384 - len(body))).encode()).decode()
        self.assertEqual(len(fits), 512)
        self.assertEqual(self.listed("--cursor", fits)["items"][0]["artifact_id"], "a05")
        over = base64.urlsafe_b64encode((body + " " * (387 - len(body))).encode()).decode()
        self.assertEqual(len(over), 516)
        self.stale(over)

    def test_registering_again_makes_every_old_cursor_stale_even_when_nothing_changed(self):
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt") for n in range(30)))
        self.stale(self.cursor)
        self.assertEqual(self.listed("--cursor", self.listed()["next_cursor"])["items"][0]["artifact_id"], "a20")

    def test_a_cursor_cannot_be_used_where_there_is_no_manifest(self):
        self.add(B, started_at="2026-01-11T10:00:00+00:00")
        self.stale(self.cursor, task_id=B)
        self.stale(cursor_of([B, self.revision, 0]), task_id=B)


class AncestryTests(HistoryCase):
    def chain(self, length, tip_continues=None):
        tasks = []
        for number in range(1, length + 1):
            parent = ident(number - 1) if number > 1 else tip_continues
            # The tip may continue a task that does not exist: its chain root is then given explicitly.
            extra = {"chain_root": tip_continues} if number == 1 and tip_continues else {}
            tasks.append(seeds.task(ident(number), continuation_of=parent, **extra, started_at=f"2026-01-10T10:{number // 60:02d}:{number % 60:02d}+00:00"))
        self.world(*tasks)
        return ident(length)

    def test_a_missing_ancestor_is_an_artifact_error(self):
        tip = self.chain(2, tip_continues=ident(900))
        error = self.bad("task-artifacts", tip, code="artifact_error")
        self.assertEqual(error["message"], "Task ancestry is unavailable")

    def test_a_cycle_is_an_artifact_error(self):
        self.world(seeds.task(A), seeds.task(B, started_at="2026-01-11T10:00:00+00:00"))
        self.sql("UPDATE task_facts SET continuation_of=? WHERE id=?", (B, A))
        self.sql("UPDATE task_facts SET continuation_of=? WHERE id=?", (A, B))
        for task_id in (A, B):
            error = self.bad("task-artifacts", task_id, code="artifact_error")
            self.assertEqual(error["message"], "Task ancestry is cyclic")

    def test_a_task_that_continues_itself_is_cyclic(self):
        self.world(seeds.task(A))
        self.sql("UPDATE task_facts SET continuation_of=? WHERE id=?", (A, A))
        self.assertEqual(self.bad("task-artifacts", A, code="artifact_error")["message"], "Task ancestry is cyclic")

    def test_a_chain_of_one_hundred_tasks_is_readable_and_one_more_is_not(self):
        tip = self.chain(100)
        self.assertEqual(self.good("task-artifacts", tip)["result"]["state"], "available")
        self.setUp()
        tip = self.chain(101)
        error = self.bad("task-artifacts", tip, code="artifact_error")
        self.assertEqual(error["message"], "Task ancestry exceeds authority bounds")

    def large_ancestor(self, size):
        self.world(seeds.task(A, title="t" * 150), seeds.task(B, continuation_of=A, started_at="2026-01-11T10:00:00+00:00"))
        header = b'{"task_id":"%s","group":"%s"}' % (A.encode(), b"g" * size)
        self.sql("UPDATE task_headers SET header=? WHERE id=?", (header, A))

    def test_an_ancestor_header_just_under_the_query_budget_is_served(self):
        self.large_ancestor(1_000_000)
        self.assertEqual(self.good("task-artifacts", B)["result"]["state"], "available")

    def test_an_ancestor_header_over_the_query_budget_fails_the_read_like_a_listing_does(self):
        """The coordinator's 1 MiB decision: the workflow-builder fact is read from the whole
        blob, so a blob over the budget fails the read and is never served as "not a builder"."""
        self.large_ancestor(1_300_000)
        error = self.bad("task-artifacts", B, code="state_incompatible")
        self.assertEqual(error["data"]["reason"], "infrastructure_unavailable")
        self.assertEqual(error["message"], "Task history cannot be read")
        status = self.bad("status", A, code="state_incompatible")
        self.assertEqual(status["message"], error["message"], "the failure a status of the same task gives")

    @unittest.expectedFailure
    def test_an_ancestor_header_over_the_query_budget_is_served_as_the_reference_serves_it(self):
        """Known limitation: the reference parses the whole header of an ancestor whatever its
        size; this engine's 1 MiB query budget refuses it."""
        self.large_ancestor(1_300_000)
        self.assertEqual(self.good("task-artifacts", B)["result"]["state"], "available")

    def test_the_ancestry_check_precedes_the_manifest(self):
        tip = self.chain(2, tip_continues=ident(900))
        self.seed("write_artifacts", {"task_id": tip, "items": [], "header_valid": False})
        self.bad("task-artifacts", tip, code="artifact_error")


class SummaryTests(ArtifactCase):
    def summary(self, *args, task_id=A):
        return self.good("status", task_id, *args)["task"]["artifact_summary"]

    def detail(self, task_id=A):
        return {"tool": "get_task_artifacts", "task_id": task_id}

    def test_the_summary_counts_the_manifest_and_points_at_the_detail_read(self):
        self.register(*(item(f"a{n}", f"f{n}.txt") for n in range(3)))
        self.assertEqual(self.summary(), {"count": 3, "state": "available", "detail_ref": self.detail()})

    def test_a_task_without_a_manifest_has_an_available_empty_summary(self):
        self.assertEqual(self.summary(), {"count": 0, "state": "available", "detail_ref": self.detail()})

    def test_an_unreadable_manifest_is_unavailable_but_keeps_its_reference(self):
        self.register(*(item(f"a{n:03d}", f"f{n:03d}.txt") for n in range(101)))
        self.assertEqual(self.summary(), {"count": 0, "state": "unavailable", "detail_ref": self.detail()})
        self.register(item("x", "x.txt"), header_valid=False)
        self.assertEqual(self.summary(), {"count": 0, "state": "unavailable", "detail_ref": self.detail()})

    def test_one_hundred_items_count_one_hundred(self):
        self.register(*(item(f"a{n:03d}", f"f{n:03d}.txt") for n in range(100)))
        self.assertEqual(self.summary()["count"], 100)

    def test_broken_ancestry_is_unavailable_with_no_reference(self):
        self.add(B, continuation_of=ident(900), chain_root=ident(900), started_at="2026-01-11T10:00:00+00:00")
        self.assertEqual(self.summary(task_id=B), {"count": 0, "state": "unavailable"})

    def test_batch_and_brief_carry_the_same_summary(self):
        self.register(item("x1", "a.txt"))
        expected = {"count": 1, "state": "available", "detail_ref": self.detail()}
        batch = self.good("status-batch", "--task-ids", A)
        self.assertEqual(batch["tasks"][0]["artifact_summary"], expected)
        self.assertEqual(self.summary("--brief"), expected)

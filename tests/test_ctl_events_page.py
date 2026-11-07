"""Black-box tests of the `task-events` page: rendering bounds, counting of hidden lines, gaps,
the log object with its capture sources, and the change token."""

import json

import seed_support as seeds
from history_support import HistoryCase, event
from reads_support import MISSING
from seed_support import A, B, C, D

PAGE_KEYS = {
    "task_id", "order", "kinds", "events", "older_cursor", "newer_cursor", "has_older", "has_newer",
    "span", "skipped", "gaps", "log", "change_token", "observed_at",
}
LOG_KEYS = {"state", "task_status", "output_drained", "capture_state", "final_seq"}
STUB_KEYS = {"seq", "kind", "observed_at", "truncated", "oversized", "bytes"}


class RenderTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def test_the_page_has_exactly_the_documented_keys(self):
        page = self.page(A)
        self.assertEqual(set(page), PAGE_KEYS)
        self.assertEqual(set(page["log"]), LOG_KEYS)
        self.assertEqual(page["order"], "ascending")
        self.assertEqual(page["task_id"], A)
        self.assertEqual(page["skipped"], {"malformed": 0, "oversized": 0})
        self.assertRegex(page["change_token"], r"^[0-9a-f]{64}$")
        self.assertRegex(page["observed_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{6})?\+00:00$")

    def test_a_prefix_is_echoed(self):
        page = self.page(A[:8])
        self.assertEqual(page["resolved_from"], {"input": A[:8], "task_id": A})
        self.assertEqual(set(page), PAGE_KEYS | {"resolved_from"})

    def test_the_kinds_label_is_default_all_or_the_sorted_explicit_list(self):
        self.assertEqual(self.page(A)["kinds"], "default")
        self.assertEqual(self.page(A, "--all-kinds")["kinds"], "all")
        for kinds in ("notice,assistant_text", "assistant_text,notice,notice"):
            self.assertEqual(self.page(A, "--kinds", kinds)["kinds"], ["assistant_text", "notice"])

    def test_events_carry_this_engines_envelope_and_no_storage_fields(self):
        document = self.good("task-events", A)
        for item in document["result"]["events"]:
            self.assertEqual(item["product"], "harflow")
            self.assertEqual(item["format"], "harflow.event")
            self.assertEqual(item["v"], 1)
            self.assertEqual(item["product_version"], document["product_version"])
            for hidden in ("task_id", "raw_offset", "source_ts"):
                self.assertNotIn(hidden, item)
            self.assertNotIn("truncated", item)

    def test_stored_envelope_and_source_fields_are_replaced_not_served(self):
        row = event(0, "notice", "hello", source_ts="2020-01-01", raw_offset=77)
        self.seed("append_event", {"task_id": C, **row})
        item = self.page(C)["events"][0]
        self.assertEqual(item["product_version"], self.good("task-events", C)["product_version"])
        self.assertNotIn("source_ts", item)
        self.assertNotIn("raw_offset", item)
        self.assertEqual((item["seq"], item["kind"], item["text"]), (0, "notice", "hello"))

    def render(self, **fields):
        self.seed("append_event", {"task_id": C, **event(0, "notice", **fields)})
        return self.page(C)["events"][0]

    def test_strings_over_two_thousand_characters_are_cut_with_an_ellipsis(self):
        item = self.render(text="x" * 2001)
        self.assertEqual(item["text"], "x" * 2000 + "...")
        self.assertIs(item["truncated"], True)

    def test_a_string_of_exactly_two_thousand_characters_is_kept(self):
        item = self.render(text="y" * 2000)
        self.assertEqual(item["text"], "y" * 2000)
        self.assertNotIn("truncated", item)

    def test_characters_not_bytes_decide_the_cut(self):
        item = self.render(text="\u00e9" * 2000)
        self.assertEqual(item["text"], "\u00e9" * 2000)
        self.assertNotIn("truncated", item)
        self.seed("append_event", {"task_id": D, **event(0, "notice", text="\U0001F600" * 2001)})
        self.assertEqual(self.page(D)["events"][0]["text"], "\U0001F600" * 2000 + "...")

    def test_lists_over_fifty_items_keep_fifty(self):
        item = self.render(items=list(range(51)))
        self.assertEqual(item["items"], list(range(50)))
        self.assertIs(item["truncated"], True)

    def test_a_list_of_fifty_items_is_kept(self):
        item = self.render(items=list(range(50)))
        self.assertEqual(len(item["items"]), 50)
        self.assertNotIn("truncated", item)

    def test_nesting_deeper_than_four_becomes_an_ellipsis(self):
        kept = self.render(n={"b": {"c": {"d": "leaf"}}})
        self.assertEqual(kept["n"], {"b": {"c": {"d": "leaf"}}})
        self.assertNotIn("truncated", kept)
        self.seed("append_event", {"task_id": D, **event(0, "notice", n={"b": {"c": {"d": {"e": "leaf"}}}})})
        cut = self.page(D)["events"][0]
        self.assertEqual(cut["n"], {"b": {"c": {"d": {"e": "..."}}}})
        self.assertIs(cut["truncated"], True)

    def test_observed_at_over_one_hundred_and_twenty_eight_characters_is_null(self):
        long_stamp, edge = "2026-01-10T10:00:00+00:00".ljust(129, "0"), "2026-01-10T10:00:00+00:00".ljust(128, "0")
        self.seed("append_event", {"task_id": C, "seq": 0, "kind": "notice", "observed_at": long_stamp, "fields": {}})
        self.seed("append_event", {"task_id": C, "seq": 1, "kind": "notice", "observed_at": edge, "fields": {}})
        events = self.page(C)["events"]
        self.assertEqual([item["observed_at"] for item in events], [None, edge])

    def test_a_filtered_line_is_hidden_but_counted_by_the_limit_and_the_span(self):
        page = self.page(A, "--from", "oldest", "--limit", "3")
        self.assertEqual(self.seqs(page), [0])
        self.assertEqual(page["span"], {"first_seq": 0, "last_seq": 2})
        self.assertIs(page["has_newer"], True)
        page = self.page(A, "--from", "oldest", "--limit", "4")
        self.assertEqual(self.seqs(page), [0, 3])
        self.assertEqual(page["span"], {"first_seq": 0, "last_seq": 3})
        everything = self.page(A, "--all-kinds", "--from", "oldest", "--limit", "3")
        self.assertEqual(self.seqs(everything), [0, 1, 2])

    def test_the_default_kinds_hide_only_deltas(self):
        page = self.page(A, "--limit", "200")
        self.assertEqual(self.seqs(page), [0, 3, 4, 5, 6, 7])
        self.assertEqual(self.seqs(self.page(A, "--all-kinds")), list(range(8)))
        self.assertEqual(self.seqs(self.page(A, "--kinds", "assistant_delta")), [1, 2])

    def test_a_task_without_a_log_has_an_empty_page_and_no_cursors_to_follow(self):
        page = self.page(C)
        self.assertEqual((page["events"], page["span"], page["gaps"]), ([], None, []))
        self.assertEqual((page["has_older"], page["has_newer"]), (False, False))
        self.assertIsNone(page["older_cursor"])


class GapTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def append(self, seq):
        self.seed("append_event", {"task_id": A, **event(seq, "notice", f"n{seq}")})

    def test_a_missing_sequence_number_is_an_unknown_gap(self):
        for seq in (0, 1, 5):
            self.append(seq)
        page = self.page(A)
        self.assertEqual(page["gaps"], [{"after_seq": 1, "before_seq": 5, "reason": "unknown"}])
        self.assertEqual(page["skipped"], {"malformed": 0, "oversized": 0})

    def test_a_skipped_line_before_the_jump_makes_it_unreadable_records(self):
        for seq in (0, 1):
            self.append(seq)
        self.seed("append_malformed_line", {"task_id": A})
        self.append(5)
        page = self.page(A)
        self.assertEqual(page["gaps"], [{"after_seq": 1, "before_seq": 5, "reason": "unreadable_records"}])
        self.assertEqual(page["skipped"], {"malformed": 1, "oversized": 0})
        self.assertEqual(self.seqs(page), [0, 1, 5])

    def test_consecutive_numbers_have_no_gap_even_with_a_skipped_line(self):
        self.append(0)
        self.seed("append_malformed_line", {"task_id": A})
        self.append(1)
        page = self.page(A)
        self.assertEqual(page["gaps"], [])
        self.assertEqual(page["skipped"]["malformed"], 1)

    def test_a_partial_trailing_line_is_held_back_and_the_cursor_stays_before_it(self):
        self.append(0)
        self.append(1)
        before = self.page(A)
        self.seed("append_partial_line", {"task_id": A})
        after = self.page(A)
        self.assertEqual(self.seqs(after), [0, 1])
        self.assertEqual(after["span"], before["span"])
        self.assertEqual(after["skipped"], {"malformed": 0, "oversized": 0})
        self.assertEqual(after["log"], before["log"])
        self.assertEqual(after["newer_cursor"], before["newer_cursor"])
        self.assertNotEqual(after["change_token"], before["change_token"])

    def test_a_malformed_line_alone_is_counted_and_advances_the_boundary(self):
        self.append(0)
        before = self.page(A)
        self.seed("append_malformed_line", {"task_id": A})
        page = self.page(A)
        self.assertEqual(page["skipped"], {"malformed": 1, "oversized": 0})
        self.assertEqual(self.seqs(page), [0])
        self.assertIs(page["has_newer"], False)
        self.assertNotEqual(page["newer_cursor"], before["newer_cursor"])


class LogObjectTests(HistoryCase):
    """The record decides termination; only a stored live or lost capture passes through."""

    def log(self, task_id=A):
        return self.page(task_id)["log"]

    def seeded(self, events=None, **fields):
        self.init(seeds.task(A, **fields))
        for row in events or []:
            self.seed("append_event", {"task_id": A, **row})

    def test_a_drained_terminal_task_ending_in_task_finished_is_final(self):
        self.seeded(seeds.a_events())
        self.assertEqual(self.log(), {
            "state": "final", "task_status": "completed", "output_drained": True,
            "capture_state": "observed_terminal", "final_seq": 7,
        })

    def test_each_terminal_name_with_an_exit_code_is_an_observed_terminal(self):
        for name in ("completed", "failed", "timed_out", "cancelled"):
            with self.subTest(status=name):
                self.setUp()
                self.seeded(seeds.a_events(), status=name, exit_code=1)
                self.assertEqual(self.log()["capture_state"], "observed_terminal")
                self.assertEqual(self.log()["task_status"], name)

    def test_without_the_finish_line_or_the_drain_the_log_is_incomplete(self):
        self.seeded(seeds.a_events()[:7])
        self.assertEqual((self.log()["state"], self.log()["final_seq"]), ("incomplete", None))
        self.setUp()
        self.seeded(seeds.a_events(), output_drained=False)
        self.assertEqual((self.log()["state"], self.log()["final_seq"]), ("incomplete", None))

    def test_a_running_task_without_an_exit_code_is_unknown(self):
        self.seeded(seeds.a_events()[:3], status="running", exit_code=None, finished_at=None)
        self.assertEqual(self.log(), {
            "state": "unknown", "task_status": "running", "output_drained": True,
            "capture_state": "unknown", "final_seq": None,
        })

    def test_a_terminal_status_without_an_exit_code_is_unknown(self):
        self.seeded(seeds.a_events(), exit_code=None)
        self.assertEqual(self.log()["capture_state"], "unknown")
        self.assertNotEqual(self.log()["state"], "final")

    def test_an_unlisted_status_with_an_exit_code_is_not_an_observed_terminal(self):
        self.seeded(seeds.a_events(), status="finished_somehow", exit_code=0)
        log = self.log()
        self.assertEqual((log["capture_state"], log["state"], log["final_seq"]), ("unknown", "unknown", None))

    def test_a_task_without_a_log_still_reports_its_record_values(self):
        self.seeded()
        self.assertEqual(self.log(), {
            "state": "unavailable", "task_status": "completed", "output_drained": True,
            "capture_state": "observed_terminal", "final_seq": None,
        })

    def test_a_stored_observed_terminal_on_a_running_task_is_unknown_never_final(self):
        self.seeded(seeds.a_events())
        self.assertEqual(self.log()["state"], "final")
        self.sql("UPDATE task_facts SET status='running', exit_code=NULL, finished_at=NULL, "
                 "finished_at_us=NULL WHERE id=?", (A,))
        log = self.log()
        self.assertEqual(log["task_status"], "running")
        self.assertEqual(log["capture_state"], "unknown")
        self.assertEqual(log["state"], "unknown")
        self.assertIsNone(log["final_seq"])

    def test_a_record_that_ends_after_the_stream_was_written_is_terminal_by_the_record(self):
        self.seeded(seeds.a_events()[:7], status="running", exit_code=None, finished_at=None)
        self.assertEqual(self.log()["capture_state"], "unknown")
        self.sql("UPDATE task_facts SET status='failed', exit_code=2 WHERE id=?", (A,))
        self.assertEqual(self.log()["capture_state"], "observed_terminal")
        self.assertEqual(self.log()["task_status"], "failed")


class ChangeTokenTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def token(self, task_id=A, *args, **options):
        return self.page(task_id, *args, **options)["change_token"]

    def test_the_token_is_stable_on_identical_input_and_independent_of_the_page_asked_for(self):
        first = self.token()
        self.assertEqual(self.token(), first)
        for arguments in (("--limit", "1"), ("--all-kinds",), ("--from", "oldest"), ("--kinds", "notice")):
            with self.subTest(arguments=arguments):
                self.assertEqual(self.token(A, *arguments), first)

    def test_a_task_prefix_gives_the_same_token(self):
        self.assertEqual(self.token(A[:8]), self.token())

    def test_the_token_differs_between_tasks_and_between_roots(self):
        import os
        self.assertNotEqual(self.token(A), self.token(B))
        other = os.path.join(self.base, "other")
        self.seed("seed_world", {"world": seeds.world()}, root=other)
        self.assertNotEqual(self.token(A, root=other), self.token())

    def test_an_append_changes_the_token(self):
        before = self.token(B)
        self.seed("append_event", {"task_id": B, **event(3, "notice", "later")})
        self.assertNotEqual(self.token(B), before)

    def test_a_partial_line_changes_the_token(self):
        before = self.token(B)
        self.seed("append_partial_line", {"task_id": B})
        self.assertNotEqual(self.token(B), before)

    def test_a_malformed_line_changes_the_token(self):
        before = self.token(B)
        self.seed("append_malformed_line", {"task_id": B})
        self.assertNotEqual(self.token(B), before)

    def test_a_record_only_change_that_leaves_the_log_object_unchanged_changes_the_token(self):
        before = self.token()
        log = self.page(A)["log"]
        for statement in (
            "UPDATE task_facts SET title='renamed' WHERE id=?",
            "UPDATE task_facts SET exit_code=2 WHERE id=?",
            "UPDATE task_facts SET model='other-model' WHERE id=?",
        ):
            with self.subTest(statement=statement):
                self.sql(statement, (A,))
                after = self.page(A)
                self.assertEqual(after["log"], log)
                self.assertNotEqual(after["change_token"], before)
                before = after["change_token"]

    def test_a_truncated_or_removed_log_changes_the_token(self):
        before = self.token()
        self.seed("truncate_log", {"task_id": A, "keep_events": 3})
        truncated = self.token()
        self.assertNotEqual(truncated, before)
        self.seed("remove_log", {"task_id": A})
        self.assertNotEqual(self.token(), truncated)

    def test_a_missing_task_has_no_token(self):
        self.bad("task-events", MISSING, code="unknown_task")

    def test_the_token_survives_a_read(self):
        first = self.token()
        document = json.dumps(self.page(A), sort_keys=True)
        self.assertIn(first, document)
        self.assertEqual(self.token(), first)

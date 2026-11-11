"""Black-box tests of `status --brief`: the allow-list, the size bound, the detail references and
`recent_activity` read from the stored event bodies."""

import json

import seed_support as seeds
from history_support import HistoryCase, event
from seed_support import A, B

# LIST_FIELDS plus the four extras of brief_status() in task_reads.py of HarflowMCP a426c5a.
ALLOWED = {
    "task_id", "backend", "status", "title", "group", "repo_path", "started_at", "finished_at", "session_id",
    "parent_task_id", "root_task_id", "duration_seconds", "background_idle_seconds", "output_capture",
    "input_channel", "last_captured_output_at", "recovered", "observation_mode", "observation_note", "owner",
    "dispatch_origin", "responding_server", "persisted_status", "observed_exit", "phase_slots",
    "needs_reconciliation", "process_identity_state", "needs_direct_lookup", "note", "exit_code",
    "owned_by_live_server", "workflow_run_id", "workflow_node_id",
    "supervision", "successor_tasks", "continuation_stop", "artifact_summary", "summary", "error", "outcome", "failure_diagnostic",
    "recent_activity", "detail_refs", "truncated_fields", "resolved_from",
}
LEFT_OUT = ("freedom", "model", "chain_root_task_id", "continuation_of_task_id", "output_drained")
BRIEF_BYTES = 8192


class AllowListTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def test_only_allowed_fields_are_kept_and_the_references_are_added(self):
        full = self.good("status", A)["task"]
        brief = self.good("status", A, "--brief")["task"]
        self.assertLessEqual(set(brief), ALLOWED)
        for name in LEFT_OUT:
            self.assertIn(name, full)
            self.assertNotIn(name, brief)
        self.assertEqual(brief["detail_refs"], {
            "status": {"tool": "get_task_status", "task_id": A},
            "events": {"tool": "get_task_events", "task_id": A},
        })
        self.assertEqual(brief["truncated_fields"], [])
        self.assertIsInstance(brief["recent_activity"], list)
        for name in ("task_id", "status", "needs_reconciliation", "persisted_status", "observed_exit",
                     "process_identity_state"):
            self.assertEqual(brief[name], full[name])

    def test_a_prefix_is_echoed_and_the_references_name_the_resolved_task(self):
        brief = self.good("status", A[:8], "--brief")["task"]
        self.assertEqual(brief["resolved_from"], {"input": A[:8], "task_id": A})
        self.assertEqual(brief["detail_refs"]["status"]["task_id"], A)

    def test_brief_changes_nothing_else_about_a_read(self):
        full = self.good("status", B)["task"]
        brief = self.good("status", B, "--brief")["task"]
        for name in ("status", "exit_code", "started_at", "finished_at", "title", "backend", "repo_path"):
            self.assertEqual(brief.get(name), full.get(name), name)

    def test_the_artifact_summary_is_kept_with_its_reference(self):
        brief = self.good("status", A, "--brief")["task"]
        self.assertEqual(brief["artifact_summary"], {
            "count": 0, "state": "available", "detail_ref": {"tool": "get_task_artifacts", "task_id": A}})

    def test_brief_is_only_a_status_option(self):
        done = self.read("task-events", A, "--json", "--brief")
        self.assertEqual(done.returncode, 2)


class SizeTests(HistoryCase):
    def test_a_large_record_gives_a_document_under_eight_kibibytes(self):
        big = "é中" * 60
        self.world(seeds.task(A, title=big, group=big, model=big, session_id="é" * 100, repo_path="/" + big,
                              backend=big), events={A: [event(n, "notice", big) for n in range(6)]})
        brief = self.good("status", A, "--brief")["task"]
        self.assertLessEqual(len(json.dumps(brief)), BRIEF_BYTES)
        self.assertGreater(len(json.dumps(brief)), 3000)
        for name in ("title", "group", "repo_path", "backend"):
            self.assertLessEqual(len(brief[name].encode()), 256, name)
            self.assertTrue(big.startswith(brief[name].lstrip("/")), name)
        for line in brief["recent_activity"]:
            self.assertLessEqual(len(line), 160)

    def test_scalars_are_cut_at_two_hundred_and_fifty_six_bytes_on_a_character_boundary(self):
        self.world(seeds.task(A, title="é" * 300), events={})
        title = self.good("status", A, "--brief")["task"]["title"]
        self.assertEqual(title, "é" * 128)
        self.setUp()
        self.world(seeds.task(A, title="中" * 300), events={})
        self.assertEqual(self.good("status", A, "--brief")["task"]["title"], "中" * 85)

    def test_a_scalar_that_fits_is_kept_whole(self):
        self.world(seeds.task(A, title="t" * 256), events={})
        self.assertEqual(self.good("status", A, "--brief")["task"]["title"], "t" * 256)


class RecentActivityTests(HistoryCase):
    def lines(self, *events, extra=()):
        self.world(seeds.task(A), events={A: list(events)})
        for row in extra:
            self.seed("append_event", {"task_id": A, **row})
        return self.good("status", A, "--brief")["task"]["recent_activity"]

    def test_the_last_three_meaningful_events_are_shown_oldest_first(self):
        rows = [event(n, "assistant_text", f"line {n}") for n in range(6)]
        self.assertEqual(self.lines(*rows), ["text  line 3", "text  line 4", "text  line 5"])

    def test_each_meaningful_kind_has_its_own_line(self):
        rows = [
            event(0, "user_message", "hello  there"), event(1, "notice", "stream ended"),
            event(2, "undelivered", reason="queue full"),
        ]
        self.assertEqual(self.lines(*rows), ["user  hello there", "notice  stream ended", "undelivered  queue full"])

    def test_a_tool_call_names_its_category_tool_and_detail(self):
        rows = [event(0, "tool_call", category="shell", tool="Bash", command="ls   -la\n/tmp", call_id="c1")]
        self.assertEqual(self.lines(*rows), ["shell  Bash  ls -la /tmp"])

    def test_a_successful_tool_result_is_silent_and_a_failed_one_names_its_call(self):
        rows = [
            event(0, "tool_call", category="shell", tool="Bash", command="false", call_id="c1"),
            event(1, "tool_result", call_id="c1", ok=True, output_tail="fine"),
            event(2, "tool_result", call_id="c1", ok=False, output_tail="\n  boom happened\nmore"),
            event(3, "tool_result", call_id="zz", ok=False),
        ]
        self.assertEqual(self.lines(*rows), ["shell  Bash  false", "failed  Bash: boom happened", "failed  a tool call"])

    def test_streamed_deltas_merge_into_one_line_until_the_finished_text_arrives(self):
        rows = [event(0, "assistant_delta", "Hel", message_id="m1", block_index=0),
                event(1, "assistant_delta", "lo ", message_id="m1", block_index=0),
                event(2, "assistant_delta", "other", message_id="m1", block_index=1),
                event(3, "assistant_delta", "world", message_id="m1", block_index=0)]
        self.assertEqual(self.lines(*rows), ["text…  Hello world"])
        self.setUp()
        finished = rows + [event(4, "assistant_text", "Hello world.", message_id="m1", block_index=0)]
        self.assertEqual(self.lines(*finished), ["text  Hello world."])

    def test_an_unidentified_delta_is_never_shown(self):
        self.assertEqual(self.lines(event(0, "assistant_delta", "loose"), event(1, "notice", "n")), ["notice  n"])

    def test_long_lines_are_cut_with_an_ellipsis_at_one_hundred_and_sixty_characters(self):
        lines = self.lines(event(0, "assistant_text", "w" * 500))
        # The line is cut to 159 characters plus an ellipsis, then the brief cuts it to 160 bytes
        # as the reference does, which removes the three byte ellipsis again.
        self.assertEqual(lines, ["text  " + "w" * 153])
        self.setUp()
        lines = self.lines(event(0, "assistant_delta", "x" * 300, message_id="m", block_index=0))
        self.assertEqual(lines, ["text…  …" + "x" * 148])
        self.assertEqual(len(lines[0].encode()), 160)

    def test_more_than_one_hundred_and_twenty_eight_unmeaningful_records_do_not_blank_the_summary(self):
        rows = [event(0, "notice", "first notice"), event(1, "assistant_text", "hello")]
        rows += [event(n, "usage", input_tokens=n) for n in range(2, 330)]
        self.assertEqual(self.lines(*rows), ["notice  first notice", "text  hello"])

    def test_deltas_never_blank_the_summary_either(self):
        rows = [event(0, "notice", "start")] + [event(n, "assistant_delta", "d", message_id="m", block_index=n)
                                                for n in range(1, 400)]
        lines = self.lines(*rows)
        self.assertEqual(lines[0], "notice  start")

    def test_one_bad_line_is_skipped_rather_than_blanking_the_summary(self):
        rows = [event(0, "notice", "kept"), event(1, "notice", "also kept"), event(2, "notice", "damaged")]
        self.world(seeds.task(A), events={A: rows})
        self.sql("UPDATE event_records SET body=? WHERE task=? AND seq=2", (b"{not json", A))
        self.assertEqual(self.good("status", A, "--brief")["task"]["recent_activity"], ["notice  kept", "notice  also kept"])
        self.seed("append_malformed_line", {"task_id": A})
        self.assertEqual(self.good("status", A, "--brief")["task"]["recent_activity"], ["notice  kept", "notice  also kept"])

    def test_a_body_that_is_not_an_object_is_skipped(self):
        rows = [event(0, "notice", "kept"), event(1, "notice", "damaged")]
        self.world(seeds.task(A), events={A: rows})
        self.sql("UPDATE event_records SET body=? WHERE task=? AND seq=1", (b"[1,2]", A))
        self.assertEqual(self.good("status", A, "--brief")["task"]["recent_activity"], ["notice  kept"])

    def test_only_the_last_megabyte_of_the_log_is_read(self):
        padding = [event(n, "usage", blob="b" * 9000) for n in range(1, 140)]
        old = self.lines(event(0, "notice", "too old"), *padding)
        self.assertEqual(old, [])
        self.setUp()
        near = [event(n, "usage", blob="b" * 9000) for n in range(1, 60)]
        self.assertEqual(self.lines(event(0, "notice", "recent enough"), *near), ["notice  recent enough"])

    def test_a_task_without_a_log_has_no_activity(self):
        self.world(seeds.task(A))
        self.assertEqual(self.good("status", A, "--brief")["task"]["recent_activity"], [])

    def test_the_full_status_document_has_no_recent_activity(self):
        self.world(seeds.task(A), events={A: [event(0, "notice", "x")]})
        self.assertNotIn("recent_activity", self.good("status", A)["task"])

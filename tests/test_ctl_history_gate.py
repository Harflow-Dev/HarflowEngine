"""Black-box tests of the usage rules and the caller gate of `task-events`, `task-prompt` and
`task-artifacts`: usage before authority, the direct-only gate for events and prompts, and the
artifact gate with its decision D18 (an agent, or any non-empty marker, is refused).
"""

import os

import seed_support as seeds
from ctl_support import one_document
from history_support import HistoryCase
from reads_support import AGENT_TEXT, MARKER_KEYS, MISSING, UNDECIDABLE_TEXT
from seed_support import A, B, C

LIVE = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f"
DIRECT = ("task-events", "task-prompt")
ALL = DIRECT + ("task-artifacts",)
UNAUTHORIZED = {"HARFLOW_TASK_ID": "x"}


class UsageTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def usage(self, *args, env=UNAUTHORIZED, **options):
        """A usage error exits 2 even where authority would refuse, and says nothing on stdout."""
        done = self.read(*args, env=env, **options)
        self.assertEqual(done.returncode, 2, (args, done.stdout, done.stderr))
        self.assertTrue(done.stderr.startswith("harflow-ctl"),
                        done.stderr)
        if "--json" in args:
            self.assertEqual(one_document(done)["error"]["code"], "usage")
        else:
            self.assertEqual(done.stdout, "")
        self.assertNotIn(A, done.stderr + done.stdout)
        return done.stderr

    def test_event_usage_errors_come_before_authority(self):
        for label, args in (
            ("limit zero", ("--limit", "0")), ("limit above", ("--limit", "201")),
            ("negative limit", ("--limit", "-1")), ("limit text", ("--limit", "x")),
            ("unknown kind", ("--kinds", "bogus")), ("one unknown kind", ("--kinds", "notice,bogus")),
            ("kinds with all", ("--kinds", "notice", "--all-kinds")),
            ("from with cursor", ("--from", "oldest", "--cursor", "x")),
            ("from newest with cursor", ("--from", "newest", "--cursor", "x")),
            ("bad origin", ("--from", "sideways")),
        ):
            with self.subTest(case=label):
                self.usage("task-events", A, "--json", *args)

    def test_a_missing_json_flag_is_a_usage_error_for_events_and_prompts(self):
        for command in DIRECT:
            with self.subTest(command=command):
                self.usage(command, A)

    def test_prompt_byte_bounds_are_usage_errors(self):
        for value in ("0", "131073", "-5", "many"):
            with self.subTest(max_bytes=value):
                self.usage("task-prompt", A, "--json", "--max-bytes", value)

    def test_the_ends_of_the_ranges_are_accepted(self):
        for value in ("1", "200"):
            self.page(A, "--limit", value)
        for value in ("1", "131072"):
            self.good("task-prompt", A, "--max-bytes", value)

    def test_artifact_limits_are_artifact_errors_not_usage_errors(self):
        for value in ("0", "51", "-1"):
            with self.subTest(limit=value):
                error = self.bad("task-artifacts", A, "--limit", value, code="artifact_error")
                self.assertEqual(error["message"], "Artifact limit must be between 1 and 50")
        done = self.read("task-artifacts", A, "--limit", "x")
        self.assertEqual(done.returncode, 2)

    def test_artifacts_do_not_require_json(self):
        done = self.read("task-artifacts", A)
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_usage_is_decided_before_the_root_is_looked_at(self):
        absent = os.path.join(self.base, "nothing")
        done = self.read("task-events", A, "--json", "--limit", "0", root=absent)
        self.assertEqual(done.returncode, 2)
        self.assertFalse(os.path.lexists(absent))


class DirectGateTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(C, started_at="2026-01-12T10:00:00+00:00"))

    def same_refusal(self, command, code, text, **options):
        bodies = []
        for ident in (A, MISSING, "bad/id", "zz", A[:8]):
            done = self.read(command, ident, "--json", **options)
            self.assertEqual(done.returncode, 1, (ident, done.stdout))
            bodies.append(done.stdout)
            for text_ in (done.stdout, done.stderr):
                for token in (A, C, MISSING, A[:8], "bad/id"):
                    self.assertNotIn(token, text_)
        self.assertEqual(len(set(bodies)), 1, bodies)
        self.assertIn(code, bodies[0])
        self.assertIn(text, bodies[0])

    def test_a_live_ancestor_is_an_agent_caller_for_known_and_unknown_ids_alike(self):
        self.live(LIVE)
        for command in DIRECT:
            with self.subTest(command=command):
                self.same_refusal(command, "agent_caller", AGENT_TEXT)

    def test_an_identity_that_cannot_be_decided_is_caller_undecidable(self):
        self.live(LIVE, identity="unknown")
        for command in DIRECT:
            with self.subTest(command=command):
                self.same_refusal(command, "caller_undecidable", UNDECIDABLE_TEXT)

    def test_the_refusal_wording_differs_per_path_from_successors(self):
        self.live(LIVE)
        error = self.bad("task-events", A, code="agent_caller")
        self.assertTrue(error["message"].startswith("Task history reads require a verified direct caller"))
        error = self.bad("task-list-page", "--predecessor-task-id", A, code="agent_caller")
        self.assertTrue(error["message"].startswith("Successor discovery requires"))

    def test_a_dead_identity_admits_the_caller(self):
        self.live(LIVE, identity="reused")
        for command in DIRECT:
            self.good(command, A)

    def test_any_present_marker_refuses_even_when_empty(self):
        for key in MARKER_KEYS:
            for value in ("x", ""):
                for command in DIRECT:
                    with self.subTest(key=key, value=value, command=command):
                        self.bad(command, A, code="agent_caller", env={key: value})

    def test_the_root_task_marker_is_not_one_of_the_four(self):
        for value in ("x", ""):
            for command in DIRECT:
                with self.subTest(value=value, command=command):
                    self.good(command, A, env={"HARFLOW_ROOT_TASK_ID": value})

    def test_a_damaged_root_is_caller_undecidable(self):
        self.seed("damage_task", {"task_id": C})
        for command in DIRECT:
            with self.subTest(command=command):
                self.same_refusal(command, "caller_undecidable", UNDECIDABLE_TEXT)

    def test_a_marker_comes_before_the_root_is_classified(self):
        self.seed("remove_root")
        self.seed("make_foreign_root")
        for command in DIRECT:
            with self.subTest(command=command):
                self.bad(command, A, code="agent_caller", env={"HARFLOW_TASK_ID": ""})
                self.bad(command, A, code="state_incompatible")
                self.bad(command, "bad/id", code="state_incompatible")

    def test_roots_without_a_store_answer_unknown_task_and_create_nothing(self):
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        absent = os.path.join(self.base, "nothing")
        for root in (empty, absent):
            for command in DIRECT:
                with self.subTest(root=os.path.basename(root), command=command):
                    self.bad(command, A, code="unknown_task", root=root)
                    self.bad(command, "bad/id", code="invalid_task_id", root=root)
        self.assertEqual(os.listdir(empty), [])
        self.assertFalse(os.path.lexists(absent))

    def test_ids_resolve_like_a_status_read(self):
        for command in DIRECT:
            with self.subTest(command=command):
                result = self.good(command, A[:8])["result"]
                self.assertEqual(result["resolved_from"], {"input": A[:8], "task_id": A})
                self.assertEqual(result["task_id"], A)
                self.assertNotIn("resolved_from", self.good(command, A)["result"])
                self.assertEqual(self.bad(command, "zz", code="unknown_task")["data"]["reason"], "prefix_too_short")
                self.assertEqual(self.bad(command, "nomatch0123", code="unknown_task")["data"]["reason"], "no_prefix_match")
                self.bad(command, MISSING, code="unknown_task")
                self.bad(command, "bad/id", code="invalid_task_id")

    def test_an_ambiguous_prefix_lists_its_candidates(self):
        self.add("abababab-0000-4000-8000-000000000001")
        self.add("abababab-0000-4000-8000-000000000002")
        for command in DIRECT:
            error = self.bad(command, "abababab", code="ambiguous_task_id")
            self.assertEqual(len(error["data"]["candidates"]), 2)


class ArtifactGateTests(HistoryCase):
    """Decision D18: an agent is refused its own artifacts and every authority refusal is an artifact error."""

    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(B, started_at="2026-01-11T10:00:00+00:00"))

    def test_an_agent_caller_is_refused_its_own_task_and_every_other(self):
        self.live(LIVE)
        for ident in (LIVE, LIVE[:8], A, B, MISSING, "zz"):
            with self.subTest(task=ident):
                error = self.bad("task-artifacts", ident, code="artifact_error")
                self.assertTrue(error["message"].startswith("Task artifacts are refused"), error)
                self.assertNotIn(LIVE, error["message"])

    def test_an_identity_that_cannot_be_decided_is_an_artifact_error(self):
        self.live(LIVE, identity="unknown")
        self.bad("task-artifacts", A, code="artifact_error")

    def test_a_dead_identity_admits_the_caller(self):
        self.live(LIVE, identity="reused")
        self.good("task-artifacts", A)

    def test_a_non_empty_marker_of_the_four_refuses(self):
        for key in MARKER_KEYS:
            with self.subTest(key=key):
                self.bad("task-artifacts", A, code="artifact_error", env={key: "x"})
                self.good("task-artifacts", A, env={key: ""})

    def test_a_non_empty_root_task_marker_refuses_and_an_empty_one_does_not(self):
        error = self.bad("task-artifacts", A, code="artifact_error", env={"HARFLOW_ROOT_TASK_ID": "x"})
        self.assertTrue(error["message"].startswith("Task artifacts are refused"), error)
        self.good("task-artifacts", A, env={"HARFLOW_ROOT_TASK_ID": ""})
        self.good("task-artifacts", A)

    def test_a_refusal_is_the_same_for_known_and_unknown_ids(self):
        bodies = set()
        for ident in (A, B, MISSING, "zz", "nomatch0123"):
            done = self.read("task-artifacts", ident, "--json", env={"HARFLOW_ROOT_TASK_ID": "x"})
            self.assertEqual(done.returncode, 1)
            bodies.add(done.stdout)
        self.assertEqual(len(bodies), 1, bodies)

    def test_the_syntax_of_the_id_is_checked_before_the_gate(self):
        error = self.bad("task-artifacts", "bad/id", code="artifact_error", env={"HARFLOW_ROOT_TASK_ID": "x"})
        self.assertEqual(error["message"], "not a valid task id: 'bad/id'")

    def test_unknown_ids_are_artifact_errors_and_an_unmatched_prefix_is_unknown_task(self):
        self.assertEqual(self.bad("task-artifacts", "zz", code="artifact_error")["message"], "Unknown task")
        self.assertEqual(self.bad("task-artifacts", MISSING, code="artifact_error")["message"], "Unknown task")
        error = self.bad("task-artifacts", "nomatch0123", code="unknown_task")
        self.assertEqual(error["data"]["reason"], "no_prefix_match")

    def test_a_missing_root_is_an_artifact_error_after_the_syntax_check(self):
        absent = os.path.join(self.base, "nothing")
        self.assertEqual(self.bad("task-artifacts", A, code="artifact_error", root=absent)["message"], "Unknown task")
        self.bad("task-artifacts", "bad/id", code="artifact_error", root=absent)
        self.assertFalse(os.path.lexists(absent))

    def test_a_foreign_root_is_state_incompatible_for_every_id(self):
        self.seed("remove_root")
        self.seed("make_foreign_root")
        for ident in (A, "bad/id", "zz"):
            with self.subTest(task=ident):
                self.bad("task-artifacts", ident, code="state_incompatible")

    def test_a_damaged_root_is_an_artifact_error(self):
        self.seed("damage_task", {"task_id": B})
        self.bad("task-artifacts", A, code="artifact_error")

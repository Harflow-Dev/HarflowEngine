"""Black-box tests of `recovery-activity`: the empty page, the usage rules, the managed gate and
the capture rows that decide the notice.

Literals are the page and notices of recovery_activity.py and backends/__init__.py of HarflowMCP
commit a426c5a, with the refusal codes of ctl.py at that commit.
"""

import json
import os

import seed_support as seeds
from feeds_support import FeedsCase, stamp
from reads_support import AGENT_TEXT, MARKER_KEYS, MISSING
from seed_support import A, B, C

NOT_CONFIRMED = "Transcript recovery requires confirmed capture loss"
UNSUPPORTED = "Backend transcript recovery is unsupported"
LIVE = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f"
LOST, LIVE_CAPTURE, TERMINAL, UNKNOWN = 1, 0, 2, 3
BACKENDS = "['antigravity', 'claude', 'codex', 'cursor', 'opencode', 'vibe']"


def page(*notices):
    return {"source": "backend_transcript", "display_only": True, "events": [], "next_cursor": None,
            "has_more": False, "reset": False, "notices": list(notices)}


class PageTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(B, status="running", exit_code=None, finished_at=None))

    def test_a_terminal_and_a_running_task_get_the_unconfirmed_empty_page(self):
        for task in (A, B):
            with self.subTest(task=task):
                document = self.recovery(task)
                self.assertEqual(document["recovery_activity"], page(NOT_CONFIRMED))
                self.assertEqual(document["task_id"], task)
                self.assertEqual(document["v"], 1)
                self.assertNotIn("resolved_from", document)

    def test_the_envelope_is_one_compact_line_with_or_without_json(self):
        for args in (("--json",), ()):
            done = self.read("recovery-activity", A, *args)
            self.assertEqual((done.returncode, done.stderr), (0, ""))
            self.assertEqual(done.stdout.count("\n"), 1)
            self.assertTrue(done.stdout.endswith("}\n"))
            self.assertEqual(set(json.loads(done.stdout)), {"product", "format", "v", "product_version", "task_id", "recovery_activity"})

    def test_a_cursor_and_any_limit_are_not_judged_on_the_empty_page(self):
        for args in (("--limit", "0"), ("--limit", "999"), ("--limit", "-5"), ("--cursor", "garbage"), ("--cursor", ""),
                     ("--limit", "0", "--cursor", "é")):
            with self.subTest(args=args):
                self.assertEqual(self.recovery(A, *args)["recovery_activity"], page(NOT_CONFIRMED))

    def test_a_prefix_is_echoed_as_resolved_from(self):
        document = self.recovery(A[:8])
        self.assertEqual(document["resolved_from"], {"input": A[:8], "task_id": A})
        self.assertEqual(document["task_id"], A)

    def test_ids_that_are_bad_unknown_or_ambiguous_are_refused_as_status_refuses_them(self):
        self.bad("recovery-activity", "bad/id", code="invalid_task_id")
        self.assertTrue(self.bad("recovery-activity", MISSING, code="unknown_task")["message"].startswith("unknown task_id: " + MISSING))
        self.add("abababab-0000-4000-8000-000000000001")
        self.add("abababab-0000-4000-8000-000000000002")
        self.assertEqual(len(self.bad("recovery-activity", "abababab", code="ambiguous_task_id")["data"]["candidates"]), 2)
        for ident in ("zz", "nomatch0123", "bad/id", MISSING):
            ours = self.bad("recovery-activity", ident)
            theirs = self.bad("status", ident)
            self.assertEqual((ours["code"], ours["message"], ours.get("data")), (theirs["code"], theirs["message"], theirs.get("data")))

    def test_usage_errors_exit_two(self):
        for args in ((), ("--limit", "x", A), (A, "--limit"), (A, "extra")):
            done = self.read("recovery-activity", *args, "--json")
            self.assertEqual(done.returncode, 2, args)
            self.assertEqual(json.loads(done.stdout)["error"]["code"], "usage")
        self.assertIn("the following arguments are required: task_id", self.read("recovery-activity").stderr)

    def test_a_missing_root_answers_like_status_and_creates_nothing(self):
        absent = os.path.join(self.base, "nothing")
        ours = self.bad("recovery-activity", A, code="unknown_task", root=absent)
        theirs = self.bad("status", A, code="unknown_task", root=absent)
        self.assertEqual(ours, theirs)
        self.assertIn("data", ours)
        self.assertEqual(ours["data"]["input"], A)
        self.bad("recovery-activity", "bad/id", code="invalid_task_id", root=absent)
        self.assertFalse(os.path.lexists(absent))


class CaptureTests(FeedsCase):
    def stream(self, task_id, capture, **fields):
        self.add(task_id, **fields)
        self.seed("append_event", {"task_id": task_id, **seeds.event(0, "notice", stamp(0), text="x")})
        self.sql("UPDATE event_streams SET capture=? WHERE task=?", (capture, task_id))

    def notices(self, task_id):
        return self.recovery(task_id)["recovery_activity"]["notices"]

    def setUp(self):
        super().setUp()
        self.init()

    def test_confirmed_loss_on_a_running_task_names_the_backend_rule(self):
        running = {"status": "running", "exit_code": None, "finished_at": None}
        self.stream(A, LOST, backend="codex", **running)
        self.stream(B, LOST, backend="claude", **running)
        self.assertEqual(self.notices(A), [UNSUPPORTED])
        self.assertEqual(len(self.notices(B)), 1)
        self.assertNotIn(self.notices(B)[0], (NOT_CONFIRMED, UNSUPPORTED))
        self.assertEqual(self.recovery(B)["recovery_activity"], page(self.notices(B)[0]))

    def test_every_registered_backend_but_claude_is_unsupported(self):
        running = {"status": "running", "exit_code": None, "finished_at": None}
        for number, backend in enumerate(("antigravity", "codex", "cursor", "opencode", "vibe")):
            task = "bk%02d" % number
            self.stream(task, LOST, backend=backend, **running)
            self.assertEqual(self.notices(task), [UNSUPPORTED], backend)

    def test_an_unregistered_backend_with_confirmed_loss_is_invalid_params(self):
        self.stream(A, LOST, backend="nobody", status="running", exit_code=None, finished_at=None)
        error = self.bad("recovery-activity", A, code="invalid_params")
        self.assertEqual(error["message"], "unknown backend 'nobody'; expected one of " + BACKENDS)

    def test_an_unregistered_backend_without_loss_is_not_looked_up(self):
        self.stream(A, LIVE_CAPTURE, backend="nobody", status="running", exit_code=None, finished_at=None)
        self.assertEqual(self.notices(A), [NOT_CONFIRMED])
        self.add(B, backend="nobody")
        self.assertEqual(self.notices(B), [NOT_CONFIRMED])

    def test_only_a_stored_loss_on_an_unobserved_task_counts(self):
        running = {"status": "running", "exit_code": None, "finished_at": None}
        self.stream("live1", LIVE_CAPTURE, **running)
        self.stream("term1", TERMINAL, **running)
        self.stream("unk1", UNKNOWN, **running)
        for task in ("live1", "term1", "unk1"):
            self.assertEqual(self.notices(task), [NOT_CONFIRMED], task)

    def test_a_finished_task_with_an_exit_code_is_observed_whatever_the_stream_says(self):
        self.stream(C, LOST, backend="codex")
        self.assertEqual(self.notices(C), [NOT_CONFIRMED])

    def test_a_finished_task_without_an_exit_code_keeps_the_stored_loss(self):
        self.stream(C, LOST, backend="codex", exit_code=None)
        self.assertEqual(self.notices(C), [UNSUPPORTED])


class GateTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def test_an_agent_ancestor_is_refused_for_known_and_unknown_ids_alike(self):
        self.live(LIVE)
        bodies = set()
        for ident in (A, MISSING, "zz", A[:8]):
            done = self.read("recovery-activity", ident, "--json")
            self.assertEqual(done.returncode, 1)
            self.assertEqual(json.loads(done.stdout)["error"]["code"], "workflow_read_refused")
            for token in (A, MISSING, "zz"):
                self.assertNotIn(token, done.stdout)
            bodies.add(done.stdout)
        self.assertEqual(len(bodies), 1)
        self.assertIn(AGENT_TEXT, bodies.pop())

    def test_a_non_empty_marker_refuses_and_an_empty_one_does_not(self):
        for key in MARKER_KEYS:
            with self.subTest(key=key):
                self.bad("recovery-activity", A, code="workflow_read_refused", env={key: "x"})
                self.good("recovery-activity", A, env={key: ""})

    def test_an_unverifiable_task_identity_is_refused(self):
        self.bad("recovery-activity", A, code="workflow_read_refused", env={"HARFLOW_TASK_ID": MISSING})

    def test_a_damaged_root_is_refused(self):
        self.seed("damage_task", {"task_id": A})
        self.bad("recovery-activity", A, code="workflow_read_refused")

    def test_the_id_syntax_comes_before_the_markers_and_the_root_class_before_both(self):
        self.bad("recovery-activity", "bad/id", code="invalid_task_id", env={"HARFLOW_TASK_ID": "x"})
        self.seed("remove_root")
        self.seed("make_foreign_root")
        self.bad("recovery-activity", A, code="state_incompatible", env={"HARFLOW_TASK_ID": "x"})
        self.bad("recovery-activity", A, code="state_incompatible", env={"HARFLOW_TASK_ID": ""})

    def test_a_dead_identity_admits_the_caller(self):
        self.live(LIVE, identity="reused")
        self.good("recovery-activity", A)

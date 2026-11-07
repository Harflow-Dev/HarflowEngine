"""Black-box tests of the caller gate on the store-backed reads.

A record on `os.getpid()` is a record on an ancestor of harflow-ctl, so it is a related
candidate. The identity states come from the fixture port: alive, reused (dead) and unknown
(undecidable).
"""

import os
import unittest

import seed_support as seeds
from reads_support import AGENT_TEXT, MARKER_KEYS, MISSING, UNDECIDABLE_TEXT, ReadsCase, day
from seed_support import A, B, C, D

LOW = "11111111-1111-4111-8111-111111111111"
HIGH = "ffffffff-ffff-4fff-8fff-ffffffffffff"
SUCCESSORS = ("task-list-page", "--predecessor-task-id", A)
MANAGED = (("list",), ("status", A), ("status-batch", "--task-ids", A))


class GateVerdictTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def agent(self, *args):
        error = self.bad(*args, code="workflow_read_refused")
        self.assertIn(AGENT_TEXT, error["message"])

    def undecidable(self, *args):
        error = self.bad(*args, code="workflow_read_refused")
        self.assertIn(UNDECIDABLE_TEXT, error["message"])

    def test_alive_ancestor_refuses_every_gated_path_with_its_own_code(self):
        self.live(B)
        for args in MANAGED:
            with self.subTest(args=args):
                self.agent(*args)
        error = self.bad("task-list-page", code="invalid_params")
        self.assertIn(AGENT_TEXT, error["message"])
        self.bad("task-list-page", "--task-ids", A, code="invalid_params")
        self.bad(*SUCCESSORS, code="agent_caller")

    def test_a_dead_identity_alone_admits_the_caller(self):
        self.live(B, identity="reused")
        self.assertEqual(self.ids(self.good("list")["tasks"]), [B, A])
        self.assertEqual(self.good("status", B)["task"]["process_identity_state"], "dead")
        self.assertEqual(self.good(*SUCCESSORS)["result"]["items"], [])

    def test_an_identity_that_cannot_be_decided_is_undecidable(self):
        self.live(B, identity="unknown")
        for args in MANAGED:
            with self.subTest(args=args):
                self.undecidable(*args)
        error = self.bad("task-list-page", code="invalid_params")
        self.assertIn(UNDECIDABLE_TEXT, error["message"])
        self.bad(*SUCCESSORS, code="caller_undecidable")

    def test_two_records_on_one_pid_alive_wins_in_both_id_orders(self):
        for stale, live in ((LOW, HIGH), (HIGH, LOW)):
            with self.subTest(stale=stale, live=live):
                self.setUp()
                self.live(stale, identity="reused")
                self.live(live, identity="alive")
                self.bad(*SUCCESSORS, code="agent_caller")
                self.agent("status", A)
                self.agent("list")

    def test_a_record_without_identity_does_not_poison_the_live_one(self):
        for blank, live in ((LOW, HIGH), (HIGH, LOW)):
            with self.subTest(blank=blank, live=live):
                self.setUp()
                self.live(blank, identity="unknown")
                self.live(live, identity="alive")
                self.bad(*SUCCESSORS, code="agent_caller")
                self.agent("list")

    def test_a_stale_record_does_not_hide_a_record_without_identity(self):
        for stale, blank in ((LOW, HIGH), (HIGH, LOW)):
            with self.subTest(stale=stale, blank=blank):
                self.setUp()
                self.live(stale, identity="reused")
                self.live(blank, identity="unknown")
                self.bad(*SUCCESSORS, code="caller_undecidable")

    def test_every_record_on_a_pid_gets_its_own_verdict(self):
        self.live(LOW, identity="reused")
        self.live(HIGH, identity="reused")
        self.assertEqual(self.good(*SUCCESSORS)["result"]["items"], [])
        self.assertEqual(len(self.good("list")["tasks"]), 3)

    def test_observed_terminal_rows_are_dead_without_an_identity_check(self):
        # The recorded identity is alive, but the record is observed-terminal.
        self.live(B, identity="alive", status="completed_observed")
        self.assertEqual(self.good(*SUCCESSORS)["result"]["items"], [])
        self.assertEqual(self.good("status", B)["task"]["process_identity_state"], "dead")
        # An unobserved failure keeps the candidate and the live identity refuses.
        self.live(C, identity="alive", status="failed_unobserved")
        self.bad(*SUCCESSORS, code="agent_caller")

    def test_observed_terminal_row_with_an_undecidable_identity_is_still_dead(self):
        self.live(B, identity="unknown", status="completed_observed")
        self.assertEqual(self.good(*SUCCESSORS)["result"]["items"], [])

    def test_a_record_on_a_pid_that_is_not_an_ancestor_is_not_related(self):
        child = self.sleeper()
        self.live(B, pid=child.pid, identity="alive")
        self.assertEqual(self.ids(self.good("list")["tasks"]), [B, A])
        self.assertEqual(self.good(*SUCCESSORS)["result"]["items"], [])

    def test_a_record_on_a_higher_ancestor_is_related(self):
        grand = os.getppid()
        try:
            self.live(B, pid=grand, identity="alive")
        except AssertionError:
            self.skipTest("the parent process cannot be read as a fixture identity")
        self.bad(*SUCCESSORS, code="agent_caller")

    def test_the_live_verdict_not_the_stored_blob_decides_the_projection(self):
        child = self.sleeper()
        self.live(B, pid=child.pid, identity="alive")
        self.assertEqual(self.good("status", B)["task"]["process_identity_state"], "alive")
        child.kill()
        child.wait()
        task = self.good("status", B)["task"]
        self.assertEqual(
            (task["process_identity_state"], task["status"], task["needs_reconciliation"]),
            ("dead", "unknown", True),
        )
        self.assertEqual(task["persisted_status"], "running")
        self.assertIs(task["observed_exit"], False)


class MarkerTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def paths(self):
        return {
            "status": (("status", A), "workflow_read_refused"),
            "status-batch": (("status-batch", "--task-ids", A), "workflow_read_refused"),
            "list": (("list",), "workflow_read_refused"),
            "page": (("task-list-page",), "invalid_params"),
            "ids": (("task-list-page", "--task-ids", A), "invalid_params"),
            "successors": (SUCCESSORS, "agent_caller"),
        }

    def plain(self, name, args, env, root):
        """No marker effect on a root with no store: empty answers, unknown ids."""
        if name in ("status", "successors"):
            self.bad(*args, code="unknown_task", env=env, root=root)
        else:
            self.good(*args, env=env, root=root)

    def test_a_non_empty_marker_refuses_every_path_on_a_store(self):
        for key in MARKER_KEYS:
            for name, (args, code) in self.paths().items():
                with self.subTest(key=key, path=name):
                    self.bad(*args, code=code, env={key: "x"})

    def test_an_empty_marker_reads_normally_except_on_direct_only_successors(self):
        for key in MARKER_KEYS:
            for name, (args, code) in self.paths().items():
                with self.subTest(key=key, path=name):
                    if name == "successors":
                        self.bad(*args, code=code, env={key: ""})
                    else:
                        self.good(*args, env={key: ""})

    def test_markers_also_refuse_on_absent_and_empty_roots(self):
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        for label, root in (("absent", os.path.join(self.base, "nothing")), ("empty", empty)):
            for name, (args, code) in self.paths().items():
                with self.subTest(root=label, path=name, marker="x"):
                    self.bad(*args, code=code, env={"HARFLOW_TASK_ID": "x"}, root=root)
                with self.subTest(root=label, path=name, marker="empty"):
                    if name == "successors":
                        self.bad(*args, code=code, env={"HARFLOW_DEPTH": ""}, root=root)
                    else:
                        self.plain(name, args, {"HARFLOW_DEPTH": ""}, root)
                with self.subTest(root=label, path=name, marker="none"):
                    self.plain(name, args, None, root)
        self.assertFalse(os.path.lexists(os.path.join(self.base, "nothing")))

    def test_refusal_bodies_carry_no_ids(self):
        other = "12345678-1234-4234-8234-123456789012"
        requests = (
            ("status", other), ("status", A), ("status-batch", "--task-ids", f"{A},{other}"),
            ("task-list-page", "--predecessor-task-id", other),
            ("task-list-page", "--predecessor-task-id", A), ("task-list-page", "--task-ids", A),
        )
        self.live(B)
        for env in ({"HARFLOW_TASK_ID": "x"}, None):
            for args in requests:
                with self.subTest(args=args, marker=bool(env)):
                    done = self.read(*args, "--json", env=env)
                    self.assertEqual(done.returncode, 1)
                    for text in (done.stdout, done.stderr):
                        for ident in (A, B, other, A[:8], other[:8]):
                            self.assertNotIn(ident, text)


class PrecedenceTests(ReadsCase):
    WITHHELD = {
        "items", "related_headers", "next_cursor", "has_more", "bootstrap_pending",
        "history_incomplete", "authority_incomplete", "counts_complete", "catalog_state", "note",
    }

    def damaged(self):
        self.init(seeds.task(A), seeds.task(B))
        self.seed("damage_task", {"task_id": B})

    def test_damage_outranks_a_marker_on_the_plain_page_and_explicit_ids(self):
        self.damaged()
        for args in (("task-list-page",), ("task-list-page", "--task-ids", A)):
            for env in (None, {"HARFLOW_TASK_ID": "x"}):
                with self.subTest(args=args, marker=bool(env)):
                    page = self.good(*args, env=env)["result"]
                    self.assertEqual(set(page), self.WITHHELD)
                    self.assertEqual(page["items"], [])
                    self.assertEqual(page["related_headers"], [])
                    self.assertIsNone(page["next_cursor"])
                    self.assertIs(page["has_more"], False)
                    self.assertIs(page["bootstrap_pending"], False)
                    self.assertIs(page["history_incomplete"], True)
                    self.assertIs(page["authority_incomplete"], True)
                    self.assertIs(page["counts_complete"], False)
                    self.assertEqual(set(page["catalog_state"]), {"status", "source", "reason"})
                    self.assertEqual(page["catalog_state"]["status"], "blocked")
                    self.assertEqual(page["catalog_state"]["source"], "task_catalog")
                    self.assertEqual(page["note"], page["catalog_state"]["reason"])
                    self.assertNotIn(A, str(page))
                    self.assertNotIn(B, str(page))

    def test_a_marker_outranks_damage_on_managed_paths_and_successors(self):
        self.damaged()
        env = {"HARFLOW_TASK_ID": "x"}
        for args in MANAGED:
            error = self.bad(*args, code="workflow_read_refused", env=env)
            self.assertIn(AGENT_TEXT, error["message"])
        self.bad(*SUCCESSORS, code="agent_caller", env=env)

    def test_damage_without_a_marker_is_an_undecidable_refusal(self):
        self.damaged()
        for args in MANAGED:
            error = self.bad(*args, code="workflow_read_refused")
            self.assertIn(UNDECIDABLE_TEXT, error["message"])
        self.bad("status", B, code="workflow_read_refused")
        self.bad(*SUCCESSORS, code="caller_undecidable")

    def test_successor_marker_comes_before_root_classification(self):
        self.seed("make_foreign_root")
        self.bad(*SUCCESSORS, code="agent_caller", env={"HARFLOW_TASK_ID": ""})
        self.bad(*SUCCESSORS, code="state_incompatible")
        for args in (("list",), ("status", A), ("task-list-page",)):
            self.bad(*args, code="state_incompatible", env={"HARFLOW_TASK_ID": "x"})

    def test_successor_limit_is_checked_after_authority_and_the_predecessor(self):
        self.init(seeds.task(A))
        self.live(B)
        self.bad(*SUCCESSORS, "--limit", "0", code="agent_caller")
        self.setUp()
        self.init(seeds.task(A))
        error = self.bad(*SUCCESSORS, "--limit", "0", code="invalid_params")
        self.assertIn("limit", error["message"])
        self.bad("task-list-page", "--predecessor-task-id", MISSING, "--limit", "0", code="unknown_task")
        error = self.bad(*SUCCESSORS, "--limit", "0", "--cursor", "garbage", code="invalid_params")
        self.assertIn("limit", error["message"])
        error = self.bad(*SUCCESSORS, "--cursor", "garbage", code="invalid_params")
        self.assertIn("cursor", error["message"])

    def test_explicit_ids_ignore_limit_and_cursor_and_keep_request_order(self):
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)),
                  seeds.task(C, started_at=day(12), finished_at=day(12)))
        page = self.good("task-list-page", "--task-ids", f"{C},{A},{B}", "--limit", "0",
                         "--cursor", "garbage")["result"]
        self.assertEqual(self.ids(page["items"]), [C, A, B])
        self.assertIsNone(page["next_cursor"])
        self.assertIs(page["has_more"], False)
        for flags in (["--limit", "101"], ["--limit", "1"]):
            page = self.good("task-list-page", "--task-ids", f"{B},{C}", *flags)["result"]
            self.assertEqual(self.ids(page["items"]), [B, C])

    def test_a_malformed_cursor_on_a_root_without_a_store_is_invalid_params(self):
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        for root in (os.path.join(self.base, "nothing"), empty):
            with self.subTest(root=root):
                error = self.bad("task-list-page", "--cursor", "garbage", code="invalid_params", root=root)
                self.assertIn("cursor", error["message"])
                self.good("task-list-page", "--task-ids", A, "--cursor", "garbage", root=root)

    def test_plain_page_limit_is_a_range_error_on_a_store(self):
        self.init(seeds.task(A))
        self.bad("task-list-page", "--limit", "0", code="invalid_params")
        self.bad("task-list-page", "--limit", "101", code="invalid_params")

    def test_plain_page_limit_is_checked_after_the_root(self):
        self.seed("make_foreign_root")
        self.bad("task-list-page", "--limit", "0", code="state_incompatible")

    def test_explicit_ids_resolve_in_request_order(self):
        self.init(seeds.task(A))
        self.bad("task-list-page", "--task-ids", "nomatch0123,bad/id", code="unknown_task")

    def test_status_root_is_classified_before_syntax(self):
        self.seed("make_foreign_root")
        self.bad("status", "bad/id", code="state_incompatible")
        self.bad("status", A, code="state_incompatible")
        self.setUp()
        self.init(seeds.task(A))
        self.bad("status", "bad/id", code="invalid_task_id")

    def test_batch_root_is_classified_before_the_bounds(self):
        self.seed("make_foreign_root")
        many = ",".join(f"x{n}" for n in range(101))
        self.bad("status-batch", "--task-ids", many, code="state_incompatible")
        self.bad("status-batch", "--task-ids", A, code="state_incompatible")
        self.setUp()
        self.init(seeds.task(A))
        self.bad("status-batch", "--task-ids", many, code="invalid_params")

    def test_a_damaged_root_answers_the_withheld_page_even_for_limit_zero(self):
        self.init(seeds.task(A), seeds.task(B))
        self.seed("damage_task", {"task_id": B})
        page = self.good("task-list-page", "--limit", "0")["result"]
        self.assertIs(page["authority_incomplete"], True)
        self.assertEqual(page["items"], [])

    def test_explicit_ids_report_invalid_entries_after_resolving_valid_ones(self):
        self.init(seeds.task(A))
        self.bad("task-list-page", "--task-ids", "bad/id,nomatch0123", code="unknown_task")
        self.bad("task-list-page", "--task-ids", "nomatch0123,bad/id", code="unknown_task")
        self.bad("task-list-page", "--task-ids", f"{A},bad/id", code="invalid_params")
        many = ",".join(f"unknown-id-{n:03d}" for n in range(101))
        self.bad("task-list-page", "--task-ids", many, code="unknown_task")


class SessionArmTests(ReadsCase):
    """The recorded process group of a task names the session of its commands."""

    def tamper(self, task_id, group):
        import sqlite3

        path = os.path.join(self.state, "store.sqlite3")
        db = sqlite3.connect(path)
        try:
            db.execute("UPDATE task_processes SET pgid=? WHERE task=?", (group, task_id))
            db.commit()
        finally:
            db.close()
        # Closing the last connection removes the write-ahead log a store root must keep.
        wal = path + "-wal"
        if not os.path.exists(wal):
            with open(wal, "xb"):
                pass

    def test_a_live_record_in_this_session_refuses_without_being_an_ancestor(self):
        child = self.sleeper()
        self.init(seeds.task(A))
        self.live(B, pid=child.pid, identity="alive")
        self.good(*SUCCESSORS)
        self.tamper(B, os.getsid(0))
        self.bad(*SUCCESSORS, code="agent_caller")
        self.tamper(B, os.getsid(0) + 1)
        self.good(*SUCCESSORS)

    def test_a_dead_record_in_this_session_admits_and_an_unknown_one_is_undecidable(self):
        child = self.sleeper()
        self.init(seeds.task(A))
        self.live(B, pid=child.pid, identity="reused")
        self.tamper(B, os.getsid(0))
        self.good(*SUCCESSORS)
        self.live(D, pid=child.pid, identity="unknown")
        self.tamper(D, os.getsid(0))
        self.bad(*SUCCESSORS, code="caller_undecidable")


class OrphanProcessRowTests(ReadsCase):
    """A process row whose task row is missing can never be decided, and is never invisible."""

    def ghost(self):
        self.sql(
            "INSERT INTO task_processes(task,pid,start_seconds,start_micros,boot_uuid,pgid)"
            " VALUES('ghost-task',?,NULL,NULL,NULL,NULL)", (os.getpid(),))

    def test_a_process_row_without_a_task_row_is_undecidable_on_every_gated_path(self):
        self.init(seeds.task(A))
        self.good(*SUCCESSORS)
        self.ghost()
        self.bad(*SUCCESSORS, code="caller_undecidable")
        for args in MANAGED:
            error = self.bad(*args, code="workflow_read_refused")
            self.assertIn(UNDECIDABLE_TEXT, error["message"])
        error = self.bad("task-list-page", code="invalid_params")
        self.assertIn(UNDECIDABLE_TEXT, error["message"])

    def test_a_live_record_still_wins_over_an_orphan_process_row(self):
        self.init(seeds.task(A))
        self.ghost()
        self.live(B)
        self.bad(*SUCCESSORS, code="agent_caller")

    def test_a_row_the_typed_mapping_rejects_is_state_incompatible(self):
        self.init(seeds.task(A))
        self.sql("UPDATE task_facts SET id='bad id' WHERE id=?", (A,))
        self.sql("UPDATE task_headers SET id='bad id' WHERE id=?", (A,))
        for args in (("list",), ("task-list-page",)):
            error = self.bad(*args, code="state_incompatible")
            self.assertEqual(error["data"]["reason"], "infrastructure_unavailable")

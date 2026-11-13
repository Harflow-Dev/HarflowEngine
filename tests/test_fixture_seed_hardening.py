"""The seed tool's status projection, event validation, raw-byte handling and a few refusals.

Projection expectations come from HarflowMCP a426c5a `store.py` (`outcome_unobserved`,
`_project_indexed_status`, lines 535-595 and 914-922), worked out by hand below, not from the
Rust code:

  unobserved = status == "running" or exit_code is None
  not unobserved                      -> state "dead", needs_reconciliation False
  unobserved, no recorded pid         -> identity "undecidable" -> state "uncertain"
  unobserved, pid, identity alive     -> "alive";  needs = status is terminal
  unobserved, pid, identity dead      -> "dead";   needs = status not in {cancelled, completed, timed_out}
  unobserved, pid, identity undecided -> "uncertain"; needs True
  (active and uncertain always needs reconciliation)
"""

import json
import os
import subprocess
import unittest

from seed_support import A, B, C, SeedCase, task

FIRST = {"task_id": A, "seq": 0, "kind": "task_started", "observed_at": "2026-01-10T10:00:00+00:00", "fields": {}}


def status_blob(case, task_id):
    return json.loads(case.rows("SELECT status FROM task_headers WHERE id=?", task_id)[0][0])


class ProjectionTests(SeedCase):
    def test_a_plain_task_projects_like_the_python_store_without_a_recorded_pid(self):
        table = (
            ("completed", 0, "dead", False), ("failed", 1, "dead", False), ("cancelled", 130, "dead", False),
            ("timed_out", 124, "dead", False), ("running", None, "uncertain", True),
            ("completed", None, "uncertain", True), ("failed", None, "uncertain", True),
            ("timed_out", None, "uncertain", True), ("cancelled", None, "uncertain", True),
        )
        self.initialized()
        for index, (status, exit_code, state, needs) in enumerate(table):
            task_id = "%08d-0000-4000-8000-000000000000" % index
            running = status == "running"
            self.ok("add_task", {"spec": task(task_id, status=status, exit_code=exit_code,
                                              finished_at=None if running else "2026-01-10T10:05:00+00:00",
                                              output_drained=not running)}, expect="{}\n")
            blob = status_blob(self, task_id)
            self.assertEqual((blob["process_identity_state"], blob["needs_reconciliation"]), (state, needs), (status, exit_code))
            self.assertIs(blob["recovered"], True)
            self.assertEqual(blob["observation_mode"], "unknown")
            self.assertEqual((blob["owner"], blob["phase_slots"]), (None, []))

    def test_a_running_task_with_an_exit_code_is_unobserved_in_python(self):
        # Python: status == "running" makes the outcome unobserved whatever exit_code says.
        self.initialized()
        self.ok("add_task", {"spec": task(A, status="running", exit_code=0, finished_at=None, output_drained=False)}, expect="{}\n")
        blob = status_blob(self, A)
        self.assertEqual((blob["process_identity_state"], blob["needs_reconciliation"]), ("uncertain", True))

    def test_a_live_caller_projects_by_its_decided_identity_and_status(self):
        table = {
            ("running", "alive"): ("alive", False), ("running", "reused"): ("dead", True), ("running", "unknown"): ("uncertain", True),
            ("failed_unobserved", "alive"): ("alive", True), ("failed_unobserved", "reused"): ("dead", True),
            ("failed_unobserved", "unknown"): ("uncertain", True),
            ("completed_observed", "alive"): ("dead", False), ("completed_observed", "reused"): ("dead", False),
            ("completed_observed", "unknown"): ("dead", False),
        }
        self.initialized()
        for index, ((status, identity), (state, needs)) in enumerate(table.items()):
            task_id = "%08d-1111-4111-8111-111111111111" % index
            self.ok("add_live_caller", {"task_id": task_id, "pid": os.getpid(), "identity": identity, "status": status, "marker": None},
                    expect='{"supported":true}\n')
            blob = status_blob(self, task_id)
            self.assertEqual((blob["process_identity_state"], blob["needs_reconciliation"]), (state, needs), (status, identity))

    def test_alive_for_a_pid_that_is_not_running_is_a_hard_error(self):
        # A pid/identity mismatch cannot be produced through the process: the identity is read for
        # the same pid. The reachable hard error is an identity that cannot be read at all.
        done = subprocess.Popen(["/usr/bin/true"])
        done.wait()
        self.initialized()
        before = self.digest()
        self.refused("add_live_caller", {"task_id": B, "pid": done.pid, "identity": "alive", "status": "running", "marker": None})
        self.assertEqual(self.digest(), before)


class ValidationTests(SeedCase):
    def event(self, **overrides):
        row = dict(FIRST, fields={})
        row.update(overrides)
        return row

    def test_invalid_events_are_refused_commit_nothing_and_the_next_append_works(self):
        self.initialized(task(A))
        invalid = (
            self.event(seq=2 ** 63), self.event(seq=2 ** 64 - 1), self.event(kind="k" * 129),
            self.event(fields={"text": "x" * 16400}),
        )
        for row in invalid:
            before = self.digest()
            self.refused("append_event", row, message="InvalidEventQuery")
            self.assertEqual(self.digest(), before)
            self.assertEqual(self.rows("SELECT count(*) FROM event_streams"), [(0,)])
        self.ok("append_event", self.event(seq=1, kind="k" * 128))
        self.assertEqual(self.rows("SELECT seq FROM event_records WHERE task=?", A), [(1,)])
        before = self.digest()
        self.refused("append_event", self.event(seq=2, kind="k" * 129), message="InvalidEventQuery")
        self.assertEqual(self.digest(), before)
        self.ok("append_event", self.event(seq=2 ** 63 - 1))
        self.assertEqual(self.rows("SELECT seq FROM event_records WHERE task=? ORDER BY start", A), [(1,), (2 ** 63 - 1,)])
        before = self.digest()
        self.refused("append_event", self.event(seq=2), message="EventOutOfOrder")
        self.assertEqual(self.digest(), before)

    def test_a_present_artifact_item_with_an_observation_is_refused(self):
        self.initialized(task(A))
        item = {"artifact_id": "x", "name": "a.txt", "target": "present", "kind": "k", "note": "n", "registered_at": "t",
                "provenance_method": "engine_output", "observation": {"device": 1, "inode": 2, "size": 3, "mtime_ns": 4}}
        before = self.digest()
        self.refused("write_artifacts", {"task_id": A, "items": [item]}, message="carries no observation")
        self.assertEqual(self.digest(), before)


class RawByteTests(SeedCase):
    def symlink_digest(self, name, target):
        root = os.path.join(self.base, name)
        os.mkdir(root)
        os.symlink(target, os.path.join(os.fsencode(root), b"link"))
        return self.digest(root=root)

    def test_symlink_targets_that_differ_only_in_non_utf8_bytes_give_different_digests(self):
        first = self.symlink_digest("one", b"\xff")
        second = self.symlink_digest("two", b"\xfe")
        self.assertNotEqual(first, second)
        self.assertEqual(first, self.symlink_digest("three", b"\xff"))

    def test_names_that_differ_only_in_non_utf8_bytes_give_different_digests(self):
        digests = []
        for index, name in enumerate((b"\xff", b"\xfe")):
            root = os.path.join(self.base, "names%d" % index)
            os.mkdir(root)
            try:
                with open(os.path.join(os.fsencode(root), name), "wb") as stream:
                    stream.write(b"same")
            except OSError as error:
                self.skipTest(f"this file system refuses non-UTF-8 names: {error}")
            digests.append(self.digest(root=root))
        self.assertNotEqual(*digests)

    def test_a_non_utf8_root_is_never_turned_into_a_different_path(self):
        lossy = os.path.join(self.base, "state�")
        os.mkdir(lossy)
        with open(os.path.join(lossy, "keep.txt"), "wb") as stream:
            stream.write(b"keep")
        raw = os.path.join(os.fsencode(self.base), b"state\xff")
        for op in ("remove_root", "make_foreign_root", "initialize_empty"):
            self.call(op, root=raw)
        self.assertEqual(sorted(os.listdir(lossy)), ["keep.txt"])
        self.assertEqual(sorted(name for name in os.listdir(self.base)), ["state�"])

    def test_a_non_utf8_operation_name_is_refused(self):
        done = self.call(b"root_state\xff")
        self.assertEqual((done.returncode, done.stdout), (1, b""))
        self.assertTrue(done.stderr.startswith(b"hf-fixture-seed: usage"), done.stderr)

    def test_real_workspace_under_a_non_utf8_parent_is_refused_where_such_a_parent_can_exist(self):
        parent = os.path.join(os.fsencode(self.base), b"par\xffent")
        try:
            os.mkdir(parent)
        except OSError as error:
            self.skipTest(f"this file system refuses non-UTF-8 names: {error}")
        root = os.path.join(parent, b"state")
        self.ok("initialize_empty", root=root)
        self.refused("add_task", {"spec": task(A, real_workspace=True)}, root=root, message="UTF-8")


if __name__ == "__main__":
    unittest.main()

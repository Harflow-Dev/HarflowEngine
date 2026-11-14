"""Black-box tests of `harflow-ctl state-inspect` over the Rust recovery record.

`store.recovery` is `harflow-recovery-1\\n<operation_id>\\n<pending|committed>\\n`. A
`committed` record for the journal's operation commits that journal in any phase; a
`pending` record closes the root. The record bytes are written here directly.
"""

import os
import unittest

from ctl_rows import JOURNAL, OPERATION
from ctl_support import CtlCase, journal, rust_root

RECORD = "store.recovery"
OTHER = "fedcba9876543210fedcba9876543210"
PHASES = ("planned", "prepared", "backed_up", "committed", "completed")
UNFINISHED = PHASES[:-1]


def record(state, operation=OPERATION):
    return f"harflow-recovery-1\n{operation}\n{state}\n"


class RecoveryRecordInspectTests(CtlCase):
    def build(self, phase=None, state=None, operation=OPERATION):
        rust_root(self.state)
        if phase is not None:
            self.write(JOURNAL, journal(self.state, phase))
        if state is not None:
            self.write(RECORD, record(state, operation))

    def verdict(self):
        result = self.inspect()
        self.assertIs(result["recovery_supported"], False)
        self.assertNotIn("recovery", result)
        return result["status"], result["reason"]

    def test_committed_record_commits_the_journal_in_every_phase(self):
        for phase in PHASES:
            with self.subTest(phase=phase):
                self.setUp()
                try:
                    self.build(phase, "committed")
                    status, reason = self.verdict()
                    self.assertNotEqual(status, "interrupted")
                    self.assertEqual((status, reason), ("ready", "compatible"))
                finally:
                    self.cleanup()

    def test_pending_record_leaves_the_journal_interrupted(self):
        for phase in UNFINISHED:
            with self.subTest(phase=phase):
                self.setUp()
                try:
                    self.build(phase, "pending")
                    self.assertEqual(self.verdict(), ("interrupted", "recovery_interrupted"))
                finally:
                    self.cleanup()
        self.setUp()
        self.build("completed", "pending")
        self.assertEqual(self.verdict(), ("incompatible", "recovery_interrupted"))

    def test_pending_record_without_a_journal_is_recovery_interrupted(self):
        self.build(None, "pending")
        self.assertEqual(self.verdict(), ("incompatible", "recovery_interrupted"))

    def test_record_for_another_operation_commits_nothing(self):
        for phase in PHASES:
            for state in ("committed", "pending"):
                with self.subTest(phase=phase, state=state):
                    self.setUp()
                    try:
                        self.build(phase, state, OTHER)
                        status, reason = self.verdict()
                        self.assertEqual(reason, "recovery_interrupted")
                        # A pending record of another operation closes the gate in
                        # every phase; a committed one leaves the journal's own verdict.
                        expected = (
                            "incompatible" if phase == "completed" or state == "pending"
                            else "interrupted"
                        )
                        self.assertEqual(status, expected)
                    finally:
                        self.cleanup()

    def test_committed_record_without_a_journal_is_ready(self):
        self.build(None, "committed")
        self.assertEqual(self.verdict(), ("ready", "compatible"))

    def test_journals_without_a_record_keep_their_e1_verdicts(self):
        # Without a record, the same journals keep their E1 verdicts.
        for phase in PHASES:
            with self.subTest(phase=phase):
                self.setUp()
                try:
                    self.build(phase)
                    expected = (
                        ("incompatible", "recovery_interrupted") if phase == "completed"
                        else ("interrupted", "recovery_interrupted")
                    )
                    self.assertEqual(self.verdict(), expected)
                finally:
                    self.cleanup()

    def test_malformed_or_non_regular_record_is_an_invalid_marker(self):
        cases = {
            "garbage": b"\x00garbage",
            "empty": b"",
            "unknown state": record("done").encode(),
            "uppercase operation": record("pending", OPERATION.upper()).encode(),
            "missing final newline": record("pending").encode()[:-1],
            "oversized": record("pending").encode() + b"x" * 128,
        }
        for label, data in cases.items():
            with self.subTest(case=label):
                self.setUp()
                try:
                    self.build()
                    self.write(RECORD, data)
                    self.assertEqual(self.verdict(), ("incompatible", "invalid_marker"))
                finally:
                    self.cleanup()
        self.setUp()
        self.build()
        os.mkdir(os.path.join(self.state, RECORD))
        self.assertEqual(self.verdict(), ("incompatible", "invalid_marker"))

    def test_record_that_is_a_symlink_is_an_invalid_marker(self):
        self.build()
        target = self.write("pending-record", record("pending"), root=self.base)
        self.symlink(target, os.path.join(self.state, RECORD))
        self.assertEqual(self.verdict(), ("incompatible", "invalid_marker"))

    def test_unreadable_record_is_infrastructure_unavailable(self):
        if os.geteuid() == 0:
            self.skipTest("permission bits do not bind the superuser")
        self.build(None, "committed")
        path = os.path.join(self.state, RECORD)
        os.chmod(path, 0)
        try:
            self.assertEqual(self.verdict(), ("incompatible", "infrastructure_unavailable"))
        finally:
            os.chmod(path, 0o600)

    def test_commit_lost_fence_is_infrastructure_unavailable(self):
        self.build()
        generation = "a" * 32
        self.write("store.fence", f"harflow-fence-1\n{generation}\ncommit_lost\n")
        self.assertEqual(self.verdict(), ("incompatible", "infrastructure_unavailable"))


if __name__ == "__main__":
    unittest.main()

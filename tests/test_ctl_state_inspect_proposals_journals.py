"""Recovery proposals of `harflow-ctl state-inspect` (plan 3.3): interrupted journals and resumable proposals."""

import unittest

from proposal_case import *  # noqa: F401,F403
from proposal_case import ProposalCase


class InterruptedJournalTests(ProposalCase):
    def test_interrupted_journal_of_our_engine_is_re_encoded_resumable_with_its_phase(self):
        def check(phase):
            proposal = self.unfinished_journal(phase)
            result = self.inspect()
            self.assertEqual((result["status"], result["reason"]), ("interrupted", "recovery_interrupted"))
            assert_proposal(self, result, resumable=True, phase=phase, expected=proposal)
            recovery = result["recovery"]
            self.assertEqual(recovery["operation_id"], OPERATION)
            self.assertEqual(recovery["backup_path"], f"{self.state}.harflow-backup-{OPERATION}")
            # The token is the canonical encoding of the journal's proposal.
            self.assertEqual(recovery["confirmation_token"], local_encode(proposal))
            if reference_available():
                self.assertEqual(recovery["confirmation_token"], reference_encode(proposal))

        self.each([(phase, phase) for phase in UNFINISHED], check)

    def test_a_resumable_journal_in_an_ineligible_root_has_no_proposal(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("FIFOs exist on Unix only")

        def unreadable_identity(case):
            case.write("store.identity", b"x")
            os.chmod(os.path.join(case.state, "store.identity"), 0)

        def lock_directory(case):
            os.mkdir(os.path.join(case.state, ".harflow-store.lock"))

        def python_state(case):
            case.write(MARKER, python_marker())

        conditions = [("lock directory", lock_directory), ("python marker", python_state)]
        if os.geteuid() != 0:
            conditions.append(("unreadable identity", unreadable_identity))

        def check(build):
            self.unfinished_journal("planned")
            build(self)
            result = self.inspect()
            assert_no_proposal(self, result)

        self.each(conditions, check)

    def test_the_publication_temporary_is_the_journal_that_counts(self):
        # Both files present: the temporary wins, as in the reference.
        main = self.unfinished_journal("planned", name=JOURNAL, operation=OTHER_OPERATION)
        pending = self.unfinished_journal("backed_up", name=JOURNAL + ".pending")
        result = self.inspect()
        assert_proposal(self, result, resumable=True, phase="backed_up", expected=pending)
        self.assertNotEqual(main["operation_id"], result["recovery"]["operation_id"])
        # Only the temporary present.
        os.remove(os.path.join(self.state, JOURNAL))
        assert_proposal(self, self.inspect(), resumable=True, phase="backed_up", expected=pending)

    def test_a_resumed_proposal_is_not_regenerated(self):
        proposal = self.unfinished_journal("prepared")
        first, second = self.inspect(), self.inspect()
        self.assertEqual(first["recovery"], second["recovery"])
        self.assertEqual(first["recovery"]["operation_id"], proposal["operation_id"])

    def test_the_journals_own_proposal_wins_even_when_the_root_has_changed_since(self):
        proposal = self.unfinished_journal("prepared")
        self.write("later", b"x")
        os.utime(os.path.join(self.state, "stray"), (1, 1))
        decoded = assert_proposal(self, self.inspect(), resumable=True, phase="prepared", expected=proposal)
        self.assertEqual(decoded["inventory_sha256"], "c" * 64)

    def test_other_engine_version_journal_has_no_proposal(self):
        def check(version):
            for phase in UNFINISHED:
                with self.subTest(version=version, phase=phase):
                    self.setUp()
                    try:
                        self.unfinished_journal(phase, version=version)
                        result = self.inspect()
                        self.assertEqual(result["status"], "interrupted")
                        assert_no_proposal(self, result)
                    finally:
                        self.cleanup()

        for version in ("0.0.11", "0.0.14", "0.0.13+python", ""):
            check(version)

    def test_a_finished_journal_of_another_engine_is_refused_without_a_proposal(self):
        self.unfinished_journal("completed", version="0.0.14")
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "recovery_interrupted"))
        assert_no_proposal(self, result)

    def test_a_journal_without_the_full_proposal_has_no_proposal(self):
        self.stray()
        self.write(JOURNAL, journal(self.state, "planned"))
        result = self.inspect()
        self.assertEqual(result["status"], "interrupted")
        assert_no_proposal(self, result)

    def test_a_journal_for_another_root_or_of_another_shape_has_no_proposal(self):
        for label, overrides in (
            ("other state_root", {"state_root": "/elsewhere"}),
            ("bad operation id", {"operation_id": "NOT-AN-ID"}),
            ("unknown phase", {"phase": "finished"}),
            ("inventory not an object", {"inventory": []}),
        ):
            with self.subTest(case=label):
                self.setUp()
                try:
                    self.stray()
                    proposal = {
                        "state_root": self.state, "engine_version": self.engine_version(),
                        "root_identity": [1, 2], "lock_identity": None,
                        "inventory_sha256": "c" * 64, "operation_id": OPERATION,
                        "prior_journal_sha256": None,
                    }
                    fields = {"phase": "planned", **overrides}
                    self.write(JOURNAL, journal(self.state, proposal=proposal, **fields))
                    assert_no_proposal(self, self.inspect())
                finally:
                    self.cleanup()


if __name__ == "__main__":
    unittest.main()

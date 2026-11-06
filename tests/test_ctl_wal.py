"""state-inspect and the `-wal` precondition of the read path (S2 amendment)."""

import os
import unittest

from ctl_rows import JOURNAL, make_dirs
from ctl_support import CtlCase, journal, rust_root, snapshot, storage_constants

WAL = "store.sqlite3-wal"
UNREADABLE = "State root cannot be read"


class WalRowTests(CtlCase):
    def wal(self):
        return os.path.join(self.state, WAL)

    def assert_unreadable(self):
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "infrastructure_unavailable"))
        self.assertEqual(result["message"], UNREADABLE)
        self.assertNotIn("recovery", result)
        return result

    def test_baseline_a_root_with_its_wal_is_ready(self):
        rust_root(self.state)
        self.assertTrue(os.path.isfile(self.wal()))
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("ready", "compatible"))

    def test_missing_wal_makes_a_current_root_unreadable(self):
        rust_root(self.state)
        os.remove(self.wal())
        self.assert_unreadable()

    def test_wal_that_is_a_directory_is_unreadable(self):
        rust_root(self.state)
        os.remove(self.wal())
        os.mkdir(self.wal())
        self.assert_unreadable()

    def test_wal_that_is_a_symlink_is_unreadable(self):
        rust_root(self.state)
        os.remove(self.wal())
        target = os.path.join(self.base, "other-wal")
        with open(target, "wb"):
            pass
        self.symlink(target, self.wal())
        self.assert_unreadable()

    def test_wal_that_is_a_fifo_is_unreadable_without_hanging(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("FIFOs exist on Unix only")
        rust_root(self.state)
        os.remove(self.wal())
        os.mkfifo(self.wal())
        self.assertEqual(self.inspect(timeout=10)["reason"], "infrastructure_unavailable")

    def test_missing_wal_does_not_hide_a_newer_schema(self):
        rust_root(self.state, identity_version=storage_constants() + 1)
        os.remove(self.wal())
        self.assertEqual(self.inspect()["reason"], "newer_store")

    def test_missing_wal_does_not_hide_a_foreign_database(self):
        rust_root(self.state, application_id=1)
        os.remove(self.wal())
        self.assertEqual(self.inspect()["reason"], "foreign_store")

    def test_journal_precedence_holds_without_a_wal(self):
        rust_root(self.state)
        os.remove(self.wal())
        self.write(JOURNAL, journal(self.state, "planned"))
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("interrupted", "recovery_interrupted"))

    def test_empty_and_lock_only_roots_need_no_wal(self):
        make_dirs(self)
        self.assertEqual(self.inspect()["status"], "ready")
        self.write(".harflow-store.lock", b"")
        self.assertEqual(self.inspect()["status"], "ready")

    def test_truncated_listing_cannot_hide_a_missing_wal(self):
        rust_root(self.state)
        for number in range(300):
            self.write(f"entry-{number:03d}", b"")
        self.assertEqual(self.inspect()["status"], "ready")
        os.remove(self.wal())
        self.assert_unreadable()

    def test_inspection_changes_nothing_in_any_wal_state(self):
        for name in ("present", "missing", "directory"):
            with self.subTest(wal=name):
                self.setUp()
                try:
                    rust_root(self.state)
                    if name != "present":
                        os.remove(self.wal())
                    if name == "directory":
                        os.mkdir(self.wal())
                    before = snapshot(self.base)
                    self.inspect()
                    self.assertEqual(snapshot(self.base), before)
                    self.assertEqual(os.path.lexists(self.wal()), name != "missing")
                finally:
                    self.cleanup()


if __name__ == "__main__":
    unittest.main()

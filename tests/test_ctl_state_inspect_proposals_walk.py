"""Recovery proposals of `harflow-ctl state-inspect` (plan 3.3): the inventory walk, readable files and non-regular entries."""

import unittest

from proposal_case import *  # noqa: F401,F403
from proposal_case import ProposalCase


class FactWalkTests(ProposalCase):
    def populate(self, directories, files):
        for number in range(directories):
            directory = os.path.join(self.state, f"d{number:03d}")
            os.makedirs(directory)
            for index in range(files):
                os.close(os.open(os.path.join(directory, f"f{index}"), os.O_WRONLY | os.O_CREAT, 0o644))

    def test_over_100000_entries_no_proposal(self):
        # 100 directories of 999 files: exactly 100,000 entries, the largest tree accepted.
        os.makedirs(self.state)
        self.populate(100, 999)
        at_bound = self.inspect(timeout=240)
        self.assertEqual((at_bound["status"], at_bound["reason"]), ("incompatible", "unmarked_nonempty"))
        assert_proposal(self, at_bound)
        # One more entry and the walk refuses; the verdict does not move.
        os.close(os.open(os.path.join(self.state, "one-more"), os.O_WRONLY | os.O_CREAT, 0o644))
        over = self.inspect(timeout=240)
        self.assertEqual((over["status"], over["reason"]), ("incompatible", "unmarked_nonempty"))
        assert_no_proposal(self, over)

    def test_the_entry_count_includes_directories_and_nested_entries(self):
        os.makedirs(self.state)
        self.populate(100, 998)
        os.makedirs(os.path.join(self.state, "d000", "deeper"))
        # 100 directories + 99,800 files + one nested directory: still within the bound.
        assert_proposal(self, self.inspect(timeout=240), digest=False)
        for index in range(100):
            os.close(os.open(os.path.join(self.state, "d000", "deeper", f"n{index}"), os.O_WRONLY | os.O_CREAT, 0o644))
        # 100 + 99,800 + 1 + 100 = 100,001 entries.
        assert_no_proposal(self, self.inspect(timeout=240))

    def test_special_entry_has_no_proposal_and_leaves_the_verdict_unchanged(self):
        for builder in (ctl_extra.fifo_in_root, ctl_extra.socket_in_root):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    result = self.inspect(timeout=10)
                    self.assertEqual((result["status"], result["reason"]), ("incompatible", "unmarked_nonempty"))
                    assert_no_proposal(self, result)
                finally:
                    self.cleanup()

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFOs exist on Unix only")
    def test_a_special_entry_anywhere_in_the_tree_is_unsafe(self):
        self.stray()
        os.makedirs(os.path.join(self.state, "a", "b"))
        fifo = os.path.join(self.state, "a", "b", "pipe")
        os.mkfifo(fifo)
        result = self.inspect(timeout=10)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unmarked_nonempty"))
        assert_no_proposal(self, result)
        os.remove(fifo)
        assert_proposal(self, self.inspect(timeout=10))

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFOs exist on Unix only")
    def test_a_special_wal_has_no_proposal(self):
        rust_root(self.state)
        wal = os.path.join(self.state, "store.sqlite3-wal")
        os.remove(wal)
        os.mkfifo(wal)
        result = self.inspect(timeout=10)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "infrastructure_unavailable"))
        assert_no_proposal(self, result)

    def test_symlinks_recorded_not_followed(self):
        self.stray()
        outside = os.path.join(self.base, "outside")
        os.makedirs(os.path.join(outside, "inner"))
        self.write("inner/file", b"x", root=outside)
        if hasattr(os, "mkfifo"):
            os.mkfifo(os.path.join(outside, "pipe"))
        self.symlink(outside, os.path.join(self.state, "to-outside"))
        self.symlink("missing/target", os.path.join(self.state, "dangling"))
        self.symlink("loop", os.path.join(self.state, "loop"))
        self.symlink(self.state, os.path.join(self.state, "to-root"))
        self.symlink("aé\U0001f600", os.path.join(self.state, "unicode-target"))
        before = snapshot(self.base)
        result = self.inspect(timeout=10)
        # Following the link would reach the FIFO and make the root unsafe; the digest the
        # reference computes (it records the link's text) matches this engine's.
        assert_proposal(self, result)
        self.assertEqual(snapshot(self.base), before)

    def test_inspecting_with_a_proposal_changes_nothing(self):
        rust_root(self.state)
        os.remove(os.path.join(self.state, "store.sqlite3-wal"))
        self.write("sub/deep/file", b"x")
        before = snapshot(self.base)
        assert_proposal(self, self.inspect())
        self.assertEqual(snapshot(self.base), before)


class FactReadableTests(ProposalCase):
    FILES = (
        MARKER, JOURNAL, JOURNAL + ".pending", "store.identity", "store.sqlite3", "store.fence",
        "store.watermark", "store.wal-acceptance", "store.recovery",
    )

    def test_unreadable_reserved_file_has_no_proposal(self):
        if not hasattr(os, "geteuid") or os.geteuid() == 0:
            self.skipTest("permission bits do not bind this user")

        def check(name):
            self.stray()
            path = self.write(name, b"x")
            os.chmod(path, 0)
            try:
                result = self.inspect()
                # The verdict depends on the file (a store-state file is ignored while no
                # store identity exists); the lack of a proposal does not.
                self.assertEqual(result["status"], "incompatible")
                assert_no_proposal(self, result)
            finally:
                os.chmod(path, 0o600)
            # The same file readable (any content) leaves the root eligible, except that
            # a journal this engine cannot qualify is not proposed over.
            if name not in (JOURNAL, JOURNAL + ".pending"):
                assert_proposal(self, self.inspect())

        self.each([(name, name) for name in self.FILES], check)

    def test_an_unreadable_directory_inside_the_root_has_no_proposal(self):
        if not hasattr(os, "geteuid") or os.geteuid() == 0:
            self.skipTest("permission bits do not bind this user")
        self.stray()
        closed = os.path.join(self.state, "closed")
        os.makedirs(closed)
        os.chmod(closed, 0)
        try:
            result = self.inspect()
            self.assertEqual(result["status"], "incompatible")
            assert_no_proposal(self, result)
        finally:
            os.chmod(closed, 0o700)


class NonRegularReservedFileTests(ProposalCase):
    # Store-state files other than the excluded names are walked like any entry, so a
    # directory or a symbolic link in their place keeps the proposal and the verdict.
    # A non-regular excluded name (store.recovery) refuses the proposal.
    NAMES = ("store.identity", "store.sqlite3", "store.fence", "store.watermark",
             "store.wal-acceptance")

    def make(self, name, kind):
        path = os.path.join(self.state, name)
        if kind == "directory":
            os.mkdir(path)
        else:
            self.symlink(self.write("target", b"x", root=self.base), path)

    def test_non_regular_store_state_file_keeps_its_proposal_and_verdict(self):
        for name in self.NAMES:
            for kind in ("directory", "symlink"):
                with self.subTest(name=name, kind=kind):
                    self.setUp()
                    try:
                        self.stray()
                        self.make(name, kind)
                        offered = self.inspect()
                        assert_proposal(self, offered)
                        # A stray FIFO makes the walk refuse; the verdict is the same.
                        os.mkfifo(os.path.join(self.state, "stray-fifo"))
                        refused = self.inspect(timeout=10)
                        assert_no_proposal(self, refused)
                        self.assertEqual((offered["status"], offered["reason"]),
                                         (refused["status"], refused["reason"]))
                    finally:
                        self.cleanup()

    def test_non_regular_recovery_record_has_no_proposal(self):
        for kind in ("directory", "symlink"):
            with self.subTest(kind=kind):
                self.setUp()
                try:
                    self.stray()
                    self.make("store.recovery", kind)
                    assert_no_proposal(self, self.inspect())
                finally:
                    self.cleanup()

    def test_a_fifo_in_place_of_a_store_state_file_is_unsafe(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("FIFOs exist on Unix only")
        for name in self.NAMES:
            with self.subTest(name=name):
                self.setUp()
                try:
                    self.stray()
                    os.mkfifo(os.path.join(self.state, name))
                    assert_no_proposal(self, self.inspect(timeout=10))
                finally:
                    self.cleanup()


class SpecialEntryAtExcludedNameTests(ProposalCase):
    # Names the inventory skips (locks, journal, recovery record, shared-memory file) must
    # be regular files when present: a FIFO, directory or symbolic link there gives no
    # proposal.
    EXCLUDED = ("store.lock", "io.lock", "store.recovery", "store.recovery.tmp", "store.sqlite3-shm",
                ".harflow-store.lock", ".harflow-recovery.json", ".harflow-recovery.json.pending")

    def test_a_special_entry_at_an_excluded_name_has_no_proposal(self):
        offered = []
        for name in self.EXCLUDED:
            for kind in ("fifo", "directory", "symlink"):
                if kind == "fifo" and not hasattr(os, "mkfifo"):
                    continue
                self.setUp()
                try:
                    self.stray()
                    path = os.path.join(self.state, name)
                    if kind == "fifo":
                        os.mkfifo(path)
                    elif kind == "directory":
                        os.mkdir(path)
                    else:
                        self.symlink(self.write("target", b"x", root=self.base), path)
                    if "recovery" in self.inspect(timeout=10):
                        offered.append((name, kind))
                finally:
                    self.cleanup()
        self.assertEqual(offered, [])


if __name__ == "__main__":
    unittest.main()

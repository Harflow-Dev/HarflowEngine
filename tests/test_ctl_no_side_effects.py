"""state-inspect and version-info change nothing, take no lease and never panic."""

import os
import time
import unittest

import ctl_rows
from ctl_table import ROWS
from ctl_support import CtlCase, python_marker, rust_root, snapshot

try:
    import fcntl
except ImportError:  # Windows
    fcntl = None

MARKER = ".harflow-store.json"
JOURNAL = ".harflow-recovery.json"
FIFO_TIMEOUT = 15


class SnapshotBaselineTests(CtlCase):
    def test_the_snapshot_detects_creation_resize_and_mtime_changes(self):
        os.mkdir(self.state)
        path = self.write("file", b"one")
        first = snapshot(self.base)
        self.write("sidecar", b"")
        created = snapshot(self.base)
        self.assertNotEqual(created, first)
        os.remove(os.path.join(self.state, "sidecar"))
        self.write("file", b"three")
        self.assertNotEqual(snapshot(self.base), first)
        self.write("file", b"one")
        os.utime(path, ns=(1, 1))
        self.assertNotEqual(snapshot(self.base), first)


class NoSideEffectTests(CtlCase):
    def test_no_row_changes_the_tree_for_any_output_mode(self):
        for name, build, _, _ in ROWS:
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    before = snapshot(self.base)
                    for args in (("state-inspect", "--json"), ("state-inspect",)):
                        result = self.ctl(*args, root=root)
                        self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(snapshot(self.base), before)
                finally:
                    self.cleanup()

    def test_missing_root_and_its_parent_are_left_untouched(self):
        parent = os.path.join(self.base, "parent")
        root = os.path.join(parent, "state")
        self.assertEqual(self.ctl("state-inspect", "--json", root=root).returncode, 0)
        self.assertFalse(os.path.lexists(parent))
        os.mkdir(parent)
        before = snapshot(parent)
        self.assertEqual(self.ctl("state-inspect", "--json", root=root).returncode, 0)
        self.assertEqual(snapshot(parent), before)
        self.assertEqual(os.listdir(parent), [])

    def test_default_root_under_home_is_not_created(self):
        before = snapshot(self.home)
        self.assertEqual(self.ctl("state-inspect", "--json", root=None).returncode, 0)
        self.assertEqual(snapshot(self.home), before)
        self.assertEqual(os.listdir(self.home), [])

    def test_no_scratch_or_sidecar_appears_beside_a_database(self):
        rust_root(self.state)
        before = sorted(os.listdir(self.state))
        self.assertIn("store.sqlite3", before)
        self.assertIn("store.sqlite3-wal", before)
        self.assertNotIn("store.sqlite3-shm", before)
        self.assertEqual(os.path.getsize(os.path.join(self.state, "store.sqlite3-wal")), 0)
        self.assertEqual(self.inspect()["status"], "ready")
        self.assertEqual(sorted(os.listdir(self.state)), before)


@unittest.skipIf(fcntl is None, "advisory file locks are exercised on Unix only")
class LeaseFreeTests(CtlCase):
    def hold(self, path):
        descriptor = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
        self.addCleanup(os.close, descriptor)
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:  # Linux keeps record locks apart from flock; macOS treats them as one lock.
            fcntl.lockf(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            pass
        probe = os.open(path, os.O_RDWR)
        self.addCleanup(os.close, probe)
        with self.assertRaises(OSError):
            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def check_while_locked(self, lock_name, prepare):
        prepare()
        lock = os.path.join(self.state, lock_name)
        self.hold(lock)
        before = snapshot(self.base)
        started = time.monotonic()
        result = self.inspect(timeout=10)
        self.assertLess(time.monotonic() - started, 10)
        self.assertEqual((result["status"], result["reason"]), ("ready", "compatible"))
        self.assertEqual(snapshot(self.base), before)

    def test_inspect_succeeds_while_another_process_holds_the_store_lock(self):
        self.check_while_locked("store.lock", lambda: rust_root(self.state))

    def test_inspect_succeeds_while_the_python_lock_is_held(self):
        self.check_while_locked(".harflow-store.lock", lambda: os.mkdir(self.state))

    def test_inspect_succeeds_while_a_write_lock_guards_a_marked_root(self):
        def prepare():
            rust_root(self.state)
            self.write("io.lock", b"")

        self.check_while_locked("io.lock", prepare)


class NoPanicTests(CtlCase):
    def assert_clean(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("panicked", result.stderr)
        self.assertEqual(result.stderr, "")

    def classify(self, name, data):
        self.write(name, data)
        result = self.ctl("state-inspect", "--json", timeout=15)
        self.assert_clean(result)
        return self.inspect()

    def sparse(self, name, size):
        path = os.path.join(self.state, name)
        os.makedirs(self.state, exist_ok=True)
        with open(path, "wb") as handle:
            try:
                handle.truncate(size)
            except OSError as error:
                self.skipTest(f"sparse files are unavailable: {error}")
        return path

    def test_marker_over_the_bound_is_invalid(self):
        self.assertEqual(self.classify(MARKER, b" " * (1024 * 1024))["reason"], "invalid_marker")

    @unittest.skipIf(os.name == "nt", "NTFS needs an explicit sparse flag")
    def test_files_far_beyond_the_bound_are_refused_promptly(self):
        # An unbounded read of a 64 GiB sparse file cannot finish inside the timeout.
        size = 64 * 1024 ** 3
        for name, status, reason in ((MARKER, "incompatible", "invalid_marker"),
                                     (JOURNAL, "incompatible", "infrastructure_unavailable")):
            with self.subTest(name=name):
                self.setUp()
                try:
                    self.sparse(name, size)
                    started = time.monotonic()
                    result = self.inspect(timeout=10)
                    self.assertLess(time.monotonic() - started, 10)
                    self.assertEqual((result["status"], result["reason"]), (status, reason))
                finally:
                    self.cleanup()

    def test_invalid_utf8_in_a_marker(self):
        body = python_marker().encode("utf-8").replace(b"harflow.store", b"harflow\xc3\x28store")
        result = self.classify(MARKER, body)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "invalid_marker"))

    def test_deeply_nested_json_does_not_overflow_the_stack(self):
        for name, reason in ((MARKER, "invalid_marker"), (JOURNAL, "infrastructure_unavailable")):
            with self.subTest(name=name):
                self.setUp()
                try:
                    result = self.classify(name, b"[" * 10000 + b"]" * 10000)
                    self.assertEqual(result["reason"], reason)
                finally:
                    self.cleanup()

    def test_storage_files_that_are_directories_or_empty(self):
        for name in ("store.identity", "store.sqlite3", MARKER, JOURNAL):
            for as_directory in (True, False):
                with self.subTest(name=name, directory=as_directory):
                    self.setUp()
                    try:
                        rust_root(self.state)
                        path = os.path.join(self.state, name)
                        if os.path.lexists(path):
                            os.remove(path)
                        if as_directory:
                            os.mkdir(path)
                        else:
                            self.write(name, b"")
                        self.assert_clean(self.ctl("state-inspect", "--json"))
                    finally:
                        self.cleanup()

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFOs exist on Unix only")
    def test_fifo_in_place_of_a_file_is_refused_without_hanging(self):
        for name, reason in ((MARKER, "invalid_marker"), (JOURNAL, "invalid_marker"),
                             ("store.identity", "invalid_marker"), ("store.sqlite3", "invalid_marker")):
            with self.subTest(name=name):
                self.setUp()
                try:
                    if name in ("store.identity", "store.sqlite3"):
                        rust_root(self.state)
                        os.remove(os.path.join(self.state, name))
                    else:
                        os.makedirs(self.state)
                    os.mkfifo(os.path.join(self.state, name))
                    started = time.monotonic()
                    result = self.ctl("state-inspect", "--json", timeout=FIFO_TIMEOUT)
                    self.assertLess(time.monotonic() - started, FIFO_TIMEOUT)
                    self.assert_clean(result)
                    document = self.inspect()
                    self.assertEqual(document["reason"], reason)
                finally:
                    self.cleanup()

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFOs exist on Unix only")
    def test_fifo_as_the_root_is_unsafe(self):
        os.mkfifo(self.state)
        result = self.inspect(timeout=FIFO_TIMEOUT)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsafe_root"))

    @unittest.skipIf(os.name == "nt", "symbolic links need privileges on Windows")
    def test_marker_that_is_a_symlink_is_not_followed(self):
        os.makedirs(self.state)
        target = os.path.join(self.base, "outside.json")
        with open(target, "w", encoding="utf-8") as handle:
            handle.write(python_marker())
        os.symlink(target, os.path.join(self.state, MARKER))
        before = snapshot(self.base)
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "invalid_marker"))
        self.assertNotIn("observed_store", result)
        self.assertEqual(snapshot(self.base), before)

    def test_large_realistic_journals_are_accepted_when_the_marker_commits_them(self):
        for builder in (ctl_rows.committed_large_journal, ctl_rows.committed_nested_journal):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    size = os.path.getsize(os.path.join(self.state, JOURNAL))
                    if builder is ctl_rows.committed_large_journal:
                        self.assertGreater(size, 1_500_000)
                        self.assertLess(size, 1_800_000)
                    result = self.inspect()
                    self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsupported_layout"))
                finally:
                    self.cleanup()

    def test_journal_bound_is_sixteen_mebibytes(self):
        outcomes = {}
        for builder in (ctl_rows.committed_journal_at_the_bound, ctl_rows.committed_journal_over_the_bound):
            self.setUp()
            try:
                builder(self)
                result = self.inspect()
                outcomes[builder.__name__] = (
                    os.path.getsize(os.path.join(self.state, JOURNAL)), result["status"], result["reason"]
                )
            finally:
                self.cleanup()
        self.assertEqual(outcomes["committed_journal_at_the_bound"],
                         (16 * 1024 * 1024, "incompatible", "unsupported_layout"))
        self.assertEqual(outcomes["committed_journal_over_the_bound"],
                         (16 * 1024 * 1024 + 1, "incompatible", "infrastructure_unavailable"))


if __name__ == "__main__":
    unittest.main()

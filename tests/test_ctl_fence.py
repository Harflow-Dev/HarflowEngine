"""state-inspect and the verification fence `store.fence` (slice A6)."""

import os
import stat
import unittest

from ctl_rows import JOURNAL
from ctl_support import CtlCase, journal, rust_root, snapshot, storage_constants

FENCE = "store.fence"
ACCEPTANCE = "store.wal-acceptance"
WAL = "store.sqlite3-wal"
GENERATION = "a" * 32
UNREADABLE = "State root cannot be read"


def fence(state="failed", generation=GENERATION):
    return f"harflow-fence-1\n{generation}\n{state}\n".encode()


def acceptance(state, generation=GENERATION):
    return f"harflow-wal-acceptance-1\n{generation}\n{state}\n".encode()


class FenceCase(CtlCase):
    def path(self, name=FENCE):
        return os.path.join(self.state, name)

    def verdict(self, **options):
        result = self.inspect(**options)
        return result["status"], result["reason"]

    def assert_unavailable(self, **options):
        self.assertEqual(self.verdict(**options), ("incompatible", "infrastructure_unavailable"))

    def assert_invalid(self, **options):
        self.assertEqual(self.verdict(**options), ("incompatible", "invalid_marker"))

    def each(self, cases, check):
        for name, value in cases:
            with self.subTest(case=name):
                self.setUp()
                try:
                    check(value)
                finally:
                    self.cleanup()


class FenceStateTests(FenceCase):
    def test_a_root_without_a_fence_is_ready_and_stays_without_one(self):
        rust_root(self.state)
        self.assertEqual(self.verdict(), ("ready", "compatible"))
        self.assertFalse(os.path.lexists(self.path()))

    def test_a_failed_fence_for_the_current_generation_is_unavailable(self):
        rust_root(self.state)
        self.write(FENCE, fence())
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]),
                         ("incompatible", "infrastructure_unavailable"))
        self.assertEqual(result["message"], UNREADABLE)
        self.assertNotIn("recovery", result)

    def test_an_unreadable_fence_is_unavailable(self):
        if not hasattr(os, "geteuid") or os.geteuid() == 0:
            self.skipTest("permission bits do not bind this user")
        rust_root(self.state)
        path = self.write(FENCE, fence("passed", "f" * 32))
        os.chmod(path, 0)
        try:
            self.assert_unavailable()
        finally:
            os.chmod(path, stat.S_IRUSR | stat.S_IWUSR)


class FenceMalformedTests(FenceCase):
    def test_foreign_or_malformed_content_is_an_invalid_marker(self):
        valid = fence().decode()
        cases = (
            ("foreign generation", fence(generation="f" * 32)),
            ("empty", b""),
            ("wrong header", valid.replace("-1\n", "-2\n").encode()),
            ("no trailing newline", valid.rstrip("\n").encode()),
            ("extra line", (valid + "extra\n").encode()),
            ("unknown state", fence("passed")),
            ("upper case state", fence("FAILED")),
            ("crlf", valid.replace("\n", "\r\n").encode()),
            ("non-utf8", b"\xff\xfe" + fence()),
            ("129 bytes", (fence() + b"x" * 128)[:129]),
            ("large", fence() + b"x" * (1 << 20)),
        )

        def check(content):
            rust_root(self.state)
            self.write(FENCE, content)
            self.assert_invalid()

        self.each(cases, check)

    def test_a_non_regular_fence_is_an_invalid_marker(self):
        def directory():
            os.mkdir(self.path())

        def symlink_to_valid():
            target = os.path.join(self.base, "elsewhere-fence")
            with open(target, "wb") as handle:
                handle.write(fence())
            self.symlink(target, self.path())

        def dangling():
            self.symlink(os.path.join(self.base, "missing"), self.path())

        def fifo():
            if not hasattr(os, "mkfifo"):
                self.skipTest("FIFOs exist on Unix only")
            os.mkfifo(self.path())

        def check(plant):
            rust_root(self.state)
            plant()
            self.assert_invalid(timeout=10)

        self.each((("directory", directory), ("symlink", symlink_to_valid),
                   ("dangling symlink", dangling), ("fifo", fifo)), check)


class FencePrecedenceTests(FenceCase):
    def test_earlier_rows_keep_precedence_over_a_fence(self):
        def newer_schema():
            rust_root(self.state, identity_version=storage_constants() + 1)

        def foreign_database():
            rust_root(self.state, application_id=1)

        def garbled_marker():
            rust_root(self.state)
            self.write("store.identity", b"garbage")

        def interrupted():
            rust_root(self.state)
            self.write(JOURNAL, journal(self.state, "planned"))

        def check(row):
            build, expected = row
            build()
            self.write(FENCE, fence(generation="f" * 32))
            self.assertEqual(self.verdict(), expected)

        self.each((
            ("newer store", (newer_schema, ("incompatible", "newer_store"))),
            ("foreign store", (foreign_database, ("incompatible", "foreign_store"))),
            ("invalid marker", (garbled_marker, ("incompatible", "invalid_marker"))),
            ("journal", (interrupted, ("interrupted", "recovery_interrupted"))),
        ), check)

    def test_the_acceptance_record_is_judged_before_the_fence(self):
        def check(row):
            record, content, expected = row
            rust_root(self.state)
            self.write(ACCEPTANCE, record)
            self.write(FENCE, content)
            self.assertEqual(self.verdict(), expected)

        self.each((
            ("malformed record, failed fence",
             (acceptance("done"), fence(), ("incompatible", "invalid_marker"))),
            ("pending record, foreign fence",
             (acceptance("pending"), fence(generation="f" * 32),
              ("incompatible", "infrastructure_unavailable"))),
            ("accepted record, failed fence",
             (acceptance("accepted"), fence(), ("incompatible", "infrastructure_unavailable"))),
            ("accepted record, foreign fence",
             (acceptance("accepted"), fence(generation="f" * 32),
              ("incompatible", "invalid_marker"))),
        ), check)

    def test_a_missing_wal_beats_a_malformed_fence(self):
        rust_root(self.state)
        os.remove(self.path(WAL))
        self.write(FENCE, fence(generation="f" * 32))
        self.assert_unavailable()

    def test_inspection_changes_nothing_with_any_fence(self):
        def check(content):
            rust_root(self.state)
            if content is not None:
                self.write(FENCE, content)
            before = snapshot(self.base)
            self.inspect()
            self.assertEqual(snapshot(self.base), before)
            self.assertEqual(os.path.lexists(self.path()), content is not None)

        self.each((("absent", None), ("failed", fence()),
                   ("foreign", fence(generation="f" * 32)), ("malformed", b"junk")), check)


if __name__ == "__main__":
    unittest.main()

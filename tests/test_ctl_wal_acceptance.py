"""state-inspect and the lost-WAL acceptance record `store.wal-acceptance` (slice A1b)."""

import os
import stat
import unittest

from ctl_rows import JOURNAL
from ctl_support import CtlCase, journal, rust_root, snapshot, storage_constants

RECORD = "store.wal-acceptance"
WAL = "store.sqlite3-wal"
GENERATION = "a" * 32
UNREADABLE = "State root cannot be read"


def record(state, generation=GENERATION):
    return f"harflow-wal-acceptance-1\n{generation}\n{state}\n".encode()


class AcceptanceCase(CtlCase):
    def path(self, name=RECORD):
        return os.path.join(self.state, name)

    def verdict(self, **options):
        result = self.inspect(**options)
        return result["status"], result["reason"]

    def assert_unavailable(self, **options):
        self.assertEqual(self.verdict(**options), ("incompatible", "infrastructure_unavailable"))

    def assert_invalid(self, **options):
        self.assertEqual(self.verdict(**options), ("incompatible", "invalid_marker"))


class AcceptanceStateTests(AcceptanceCase):
    def test_absent_and_accepted_leave_the_root_ready(self):
        rust_root(self.state)
        self.assertEqual(self.verdict(), ("ready", "compatible"))
        self.write(RECORD, record("accepted"))
        self.assertEqual(self.verdict(), ("ready", "compatible"))

    def test_a_seeded_root_without_a_record_is_ready_and_stays_without_one(self):
        rust_root(self.state)
        self.inspect()
        self.assertFalse(os.path.lexists(self.path()))

    def test_pending_and_failed_are_unreadable(self):
        for state in ("pending", "failed"):
            with self.subTest(state=state):
                self.setUp()
                try:
                    rust_root(self.state)
                    self.write(RECORD, record(state))
                    self.assert_unavailable()
                    result = self.inspect()
                    self.assertEqual(result["message"], UNREADABLE)
                    self.assertNotIn("recovery", result)
                finally:
                    self.cleanup()

    def test_an_empty_wal_does_not_change_the_verdicts(self):
        for state, expected in (
            ("accepted", ("ready", "compatible")),
            ("pending", ("incompatible", "infrastructure_unavailable")),
            ("failed", ("incompatible", "infrastructure_unavailable")),
        ):
            with self.subTest(state=state):
                self.setUp()
                try:
                    rust_root(self.state)
                    self.assertEqual(os.path.getsize(self.path(WAL)), 0)
                    self.write(RECORD, record(state))
                    self.assertEqual(self.verdict(), expected)
                finally:
                    self.cleanup()

    def test_an_unreadable_record_is_unavailable(self):
        if not hasattr(os, "geteuid") or os.geteuid() == 0:
            self.skipTest("permission bits do not bind this user")
        rust_root(self.state)
        path = self.write(RECORD, record("accepted"))
        os.chmod(path, 0)
        try:
            self.assert_unavailable()
        finally:
            os.chmod(path, stat.S_IRUSR | stat.S_IWUSR)


class AcceptanceMalformedTests(AcceptanceCase):
    def test_malformed_content_is_an_invalid_marker(self):
        valid = record("accepted").decode()
        cases = {
            "foreign generation accepted": record("accepted", "f" * 32),
            "foreign generation pending": record("pending", "f" * 32),
            "empty": b"",
            "wrong header": valid.replace("-1\n", "-2\n").encode(),
            "no trailing newline": valid.rstrip("\n").encode(),
            "extra line": (valid + "extra\n").encode(),
            "unknown state": record("done"),
            "upper case state": record("ACCEPTED"),
            "crlf": valid.replace("\n", "\r\n").encode(),
            "non-utf8": b"\xff\xfe" + record("accepted"),
            "128 bytes": (record("accepted") + b"x" * 128)[:128],
            "129 bytes": (record("accepted") + b"x" * 128)[:129],
            "large": record("accepted") + b"x" * (1 << 20),
        }
        for name, content in cases.items():
            with self.subTest(case=name):
                self.setUp()
                try:
                    rust_root(self.state)
                    self.write(RECORD, content)
                    self.assert_invalid()
                finally:
                    self.cleanup()

    def test_a_non_regular_record_is_an_invalid_marker(self):
        def directory():
            os.mkdir(self.path())

        def symlink_to_valid():
            target = os.path.join(self.base, "elsewhere-record")
            with open(target, "wb") as handle:
                handle.write(record("accepted"))
            self.symlink(target, self.path())

        def dangling():
            self.symlink(os.path.join(self.base, "missing"), self.path())

        def fifo():
            if not hasattr(os, "mkfifo"):
                self.skipTest("FIFOs exist on Unix only")
            os.mkfifo(self.path())

        for name, plant in (
            ("directory", directory), ("symlink", symlink_to_valid),
            ("dangling symlink", dangling), ("fifo", fifo),
        ):
            with self.subTest(kind=name):
                self.setUp()
                try:
                    rust_root(self.state)
                    plant()
                    self.assert_invalid(timeout=10)
                finally:
                    self.cleanup()


class AcceptancePrecedenceTests(AcceptanceCase):
    def test_existing_rows_keep_precedence_over_a_pending_record(self):
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

        for name, build, expected in (
            ("newer store", newer_schema, ("incompatible", "newer_store")),
            ("foreign store", foreign_database, ("incompatible", "foreign_store")),
            ("invalid marker", garbled_marker, ("incompatible", "invalid_marker")),
            ("journal", interrupted, ("interrupted", "recovery_interrupted")),
        ):
            with self.subTest(row=name):
                self.setUp()
                try:
                    build()
                    self.write(RECORD, record("pending"))
                    self.assertEqual(self.verdict(), expected)
                finally:
                    self.cleanup()

    def test_a_missing_or_non_regular_wal_beats_a_malformed_record(self):
        for kind in ("missing", "directory"):
            with self.subTest(wal=kind):
                self.setUp()
                try:
                    rust_root(self.state)
                    os.remove(self.path(WAL))
                    if kind == "directory":
                        os.mkdir(self.path(WAL))
                    # Reached, this record would read as an invalid marker.
                    self.write(RECORD, record("accepted", "f" * 32))
                    self.assert_unavailable()
                finally:
                    self.cleanup()

    def test_inspection_changes_nothing_with_any_record(self):
        for name, content in (
            ("absent", None), ("accepted", record("accepted")), ("pending", record("pending")),
            ("failed", record("failed")), ("malformed", b"junk"),
        ):
            with self.subTest(record=name):
                self.setUp()
                try:
                    rust_root(self.state)
                    if content is not None:
                        self.write(RECORD, content)
                    before = snapshot(self.base)
                    self.inspect()
                    self.assertEqual(snapshot(self.base), before)
                    self.assertEqual(os.path.lexists(self.path()), content is not None)
                finally:
                    self.cleanup()


if __name__ == "__main__":
    unittest.main()

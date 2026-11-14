"""state-inspect, the store reads and the commit watermark `store.watermark` (slice A7a)."""

import os
import sqlite3
import stat
import struct
import unittest
import zlib

from ctl_rows import JOURNAL
from ctl_support import (CtlCase, journal, rust_root, snapshot, storage_constants,
                         watermark_bytes, watermark_payload)
from reads_support import ReadsCase
from seed_support import A

WATERMARK = "store.watermark"
FENCE = "store.fence"
ACCEPTANCE = "store.wal-acceptance"
WAL = "store.sqlite3-wal"
GENERATION = "a" * 32
UNAVAILABLE = ("incompatible", "infrastructure_unavailable")
INVALID = ("incompatible", "invalid_marker")
READY = ("ready", "compatible")


def fence(generation=GENERATION):
    return f"harflow-fence-1\n{generation}\nfailed\n".encode()


def torn(data, offset):
    """`data` with the payload at `offset` damaged so that its CRC no longer matches."""
    changed = bytearray(data)
    changed[offset + 40] ^= 0xFF
    return bytes(changed)


class WatermarkCase(CtlCase):
    def path(self, name=WATERMARK):
        return os.path.join(self.state, name)

    def verdict(self, **options):
        result = self.inspect(**options)
        return result["status"], result["reason"]

    def each(self, cases, check):
        for name, value in cases:
            with self.subTest(case=name):
                self.setUp()
                try:
                    check(value)
                finally:
                    self.cleanup()


class WatermarkFormatTests(WatermarkCase):
    def test_the_fixture_payload_is_the_documented_layout(self):
        payload = watermark_payload(GENERATION, 0x0102030405060708)
        self.assertEqual(len(payload), 64)
        self.assertEqual(payload[:8], b"HFWMARK1")
        self.assertEqual(payload[8:40], GENERATION.encode())
        self.assertEqual(struct.unpack("<Q", payload[40:48])[0], 0x0102030405060708)
        self.assertEqual(payload[48:60], bytes(12))
        self.assertEqual(struct.unpack("<I", payload[60:])[0], zlib.crc32(payload[:60]))
        data = watermark_bytes(GENERATION, a=3, b=None)
        self.assertEqual(len(data), 8192)
        self.assertEqual(data[64:], bytes(8192 - 64))


class WatermarkStateTests(WatermarkCase):
    def test_a_valid_watermark_is_ready_whichever_slot_is_valid(self):
        def check(content):
            rust_root(self.state, watermark=False)
            self.write(WATERMARK, content)
            self.assertEqual(self.verdict(), READY)

        whole = watermark_bytes(GENERATION, a=4, b=5)
        self.each((
            ("seq zero", watermark_bytes(GENERATION)),
            ("slot A torn", torn(whole, 0)),
            ("slot B torn", torn(whole, 4096)),
            ("slot B empty", watermark_bytes(GENERATION, a=7, b=None)),
            ("far ahead of any database", watermark_bytes(GENERATION, a=0, b=2**63)),
        ), check)

    def test_a_missing_or_slotless_watermark_is_unavailable(self):
        def check(content):
            rust_root(self.state, watermark=False)
            if content is not None:
                self.write(WATERMARK, content)
            result = self.inspect()
            self.assertEqual((result["status"], result["reason"]), UNAVAILABLE)
            self.assertNotIn("recovery", result)

        self.each((
            ("missing", None),
            ("all zero", bytes(8192)),
            ("both torn", torn(torn(watermark_bytes(GENERATION, 1, 2), 0), 4096)),
            ("filler in both blocks", bytes([1]) * 8192),
        ), check)

    def test_an_unreadable_watermark_is_unavailable(self):
        if not hasattr(os, "geteuid") or os.geteuid() == 0:
            self.skipTest("permission bits do not bind this user")
        rust_root(self.state)
        os.chmod(self.path(), 0)
        try:
            self.assertEqual(self.verdict(), UNAVAILABLE)
        finally:
            os.chmod(self.path(), stat.S_IRUSR | stat.S_IWUSR)


class WatermarkMalformedTests(WatermarkCase):
    def test_a_wrong_length_or_a_foreign_generation_is_an_invalid_marker(self):
        valid = watermark_bytes(GENERATION)

        def check(content):
            rust_root(self.state, watermark=False)
            self.write(WATERMARK, content)
            self.assertEqual(self.verdict(), INVALID)

        self.each((
            ("empty", b""),
            ("one payload", valid[:64]),
            ("one block", valid[:4096]),
            ("one byte short", valid[:8191]),
            ("one byte long", valid + b"\0"),
            ("large", valid * 200),
            ("foreign generation", watermark_bytes("f" * 32)),
            ("foreign slot B", watermark_bytes(GENERATION, a=9, b=None)[:4096]
             + watermark_bytes("f" * 32, a=1, b=None)[:4096]),
        ), check)

    def test_a_non_regular_watermark_is_an_invalid_marker(self):
        def directory():
            os.mkdir(self.path())

        def symlink_to_valid():
            target = os.path.join(self.base, "elsewhere-watermark")
            with open(target, "wb") as handle:
                handle.write(watermark_bytes(GENERATION))
            self.symlink(target, self.path())

        def dangling():
            self.symlink(os.path.join(self.base, "missing"), self.path())

        def fifo():
            if not hasattr(os, "mkfifo"):
                self.skipTest("FIFOs exist on Unix only")
            os.mkfifo(self.path())

        def check(plant):
            rust_root(self.state, watermark=False)
            plant()
            self.assertEqual(self.verdict(timeout=10), INVALID)

        self.each((("directory", directory), ("symlink", symlink_to_valid),
                   ("dangling symlink", dangling), ("fifo", fifo)), check)


class WatermarkPrecedenceTests(WatermarkCase):
    def test_earlier_rows_keep_precedence_over_a_missing_watermark(self):
        def newer_schema():
            rust_root(self.state, identity_version=storage_constants() + 1, watermark=False)

        def foreign_database():
            rust_root(self.state, application_id=1, watermark=False)

        def garbled_marker():
            rust_root(self.state, watermark=False)
            self.write("store.identity", b"garbage")

        def interrupted():
            rust_root(self.state, watermark=False)
            self.write(JOURNAL, journal(self.state, "planned"))

        def check(row):
            build, expected = row
            build()
            self.assertEqual(self.verdict(), expected)

        self.each((
            ("newer store", (newer_schema, ("incompatible", "newer_store"))),
            ("foreign store", (foreign_database, ("incompatible", "foreign_store"))),
            ("invalid marker", (garbled_marker, INVALID)),
            ("journal", (interrupted, ("interrupted", "recovery_interrupted"))),
        ), check)

    def test_the_wal_the_acceptance_record_and_the_fence_come_first(self):
        def missing_wal():
            os.remove(self.path(WAL))

        def pending_record():
            self.write(ACCEPTANCE, f"harflow-wal-acceptance-1\n{GENERATION}\npending\n")

        def foreign_fence():
            self.write(FENCE, fence("f" * 32))

        def failed_fence():
            self.write(FENCE, fence())

        def check(row):
            plant, expected = row
            rust_root(self.state)
            plant()
            self.write(WATERMARK, watermark_bytes("f" * 32))
            self.assertEqual(self.verdict(), expected)

        self.each((
            ("missing wal", (missing_wal, UNAVAILABLE)),
            ("pending acceptance record", (pending_record, UNAVAILABLE)),
            ("foreign fence", (foreign_fence, INVALID)),
            ("failed fence", (failed_fence, UNAVAILABLE)),
        ), check)

    def test_roots_without_a_store_need_no_watermark(self):
        def empty():
            os.mkdir(self.state)

        def lock_only():
            self.write(".harflow-store.lock", b"")

        def check(build):
            build()
            self.assertEqual(self.verdict(), READY)
            self.assertFalse(os.path.lexists(self.path()))

        self.each((("empty directory", empty), ("lock file only", lock_only)), check)

    def test_inspection_changes_nothing_with_any_watermark(self):
        def check(content):
            rust_root(self.state, watermark=False)
            if content is not None:
                self.write(WATERMARK, content)
            before = snapshot(self.base)
            self.inspect()
            self.assertEqual(snapshot(self.base), before)
            self.assertEqual(os.path.lexists(self.path()), content is not None)

        self.each((("absent", None), ("valid", watermark_bytes(GENERATION)),
                   ("slotless", bytes(8192)), ("foreign", watermark_bytes("f" * 32)),
                   ("short", b"junk")), check)


class WatermarkReadTests(ReadsCase):
    """Real roots built by hf-fixture-seed, read through harflow-ctl."""

    def generation(self):
        with open(os.path.join(self.state, "store.identity"), encoding="utf-8") as handle:
            return handle.read().splitlines()[2]

    def sequence(self):
        uri = "file:%s?mode=ro" % os.path.join(self.state, "store.sqlite3")
        db = sqlite3.connect(uri, uri=True)
        try:
            return db.execute("SELECT value FROM commit_sequence WHERE singleton=1").fetchone()[0]
        finally:
            db.close()

    def replace(self, data):
        path = os.path.join(self.state, WATERMARK)
        if data is None:
            os.remove(path)
            return
        with open(path, "r+b") as handle:
            handle.truncate(0)
            handle.write(data)

    def test_a_seeded_root_reads_and_its_watermark_matches_the_database(self):
        self.init()
        self.add(A)
        generation, sequence = self.generation(), self.sequence()
        self.assertGreater(sequence, 0)
        with open(os.path.join(self.state, WATERMARK), "rb") as handle:
            data = handle.read()
        self.assertEqual(len(data), 8192)
        seqs = []
        for offset in (0, 4096):
            payload = data[offset:offset + 64]
            if payload == watermark_payload(generation, struct.unpack("<Q", payload[40:48])[0]):
                seqs.append(struct.unpack("<Q", payload[40:48])[0])
        self.assertEqual(max(seqs), sequence)
        self.assertEqual(self.ids(self.good("list")["tasks"]), [A])

    def test_reads_refuse_a_missing_slotless_or_ahead_watermark(self):
        def check(row):
            build, inspected, message = row
            self.init()
            self.add(A)
            self.replace(build(self.generation(), self.sequence()))
            self.assertEqual(self.verdict(), inspected)
            for args in (("list",), ("status", A), ("task-list-page",)):
                error = self.bad(*args, code="state_incompatible")
                self.assertEqual(error["data"]["reason"], "infrastructure_unavailable", args)
                if message is not None:
                    self.assertIn(message, error["message"])

        rows = (
            ("missing", (lambda g, s: None, UNAVAILABLE, None)),
            ("slotless", (lambda g, s: bytes(8192), UNAVAILABLE, None)),
            ("ahead", (lambda g, s: watermark_bytes(g, a=s, b=s + 1), READY,
                       "acknowledged commits are missing")),
        )
        for name, row in rows:
            with self.subTest(case=name):
                self.setUp()
                try:
                    check(row)
                finally:
                    self.cleanup()

    def test_a_torn_slot_or_a_watermark_behind_the_database_still_reads(self):
        self.init()
        self.add(A)
        generation, sequence = self.generation(), self.sequence()
        for name, data in (
            ("slot B torn", torn(watermark_bytes(generation, a=sequence, b=sequence), 4096)),
            ("behind", watermark_bytes(generation, a=sequence - 1, b=None)),
            ("equal", watermark_bytes(generation, a=None, b=sequence)),
        ):
            with self.subTest(case=name):
                self.replace(data)
                self.assertEqual(self.verdict(), READY)
                self.assertEqual(self.ids(self.good("list")["tasks"]), [A])

    def verdict(self):
        result = self.inspect()
        return result["status"], result["reason"]


if __name__ == "__main__":
    unittest.main()

"""Hostile journals, non-regular entries and the descriptor-based enumeration."""

import os
import re
import subprocess
import sys
import time
import unittest

from ctl_extra import EXTRA_ROWS
from ctl_support import ROOT, SCRIPTED_ENV, CtlCase, binary, snapshot

DESCRIPTOR = ROOT / "crates" / "hf-platform" / "src" / "ctl" / "state_observer" / "descriptor.rs"
MEMORY_LIMIT = 256 * 1024 * 1024
MEASURE = (
    "import resource, subprocess, sys\n"
    "run = subprocess.run([sys.argv[1], 'state-inspect', '--json'], capture_output=True,\n"
    "                     stdin=subprocess.DEVNULL)\n"
    "peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss\n"
    "print(run.returncode, peak)\n"
)


class ExtraRowTests(CtlCase):
    # For invalid UTF-8 the reference refuses the journal but reports the decoder's own
    # reason text; this engine reports infrastructure_unavailable. Both refuse it.
    def test_each_row_has_the_reference_classification(self):
        for name, build, status, reason, _ in EXTRA_ROWS:
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    before = snapshot(self.base)
                    started = time.monotonic()
                    result = self.inspect(root, timeout=15)
                    self.assertLess(time.monotonic() - started, 15)
                    self.assertEqual((result["status"], result["reason"]), (status, reason))
                    self.assertNotIn("recovery", result)
                    self.assertEqual(snapshot(self.base), before)
                finally:
                    self.cleanup()

    def test_baselines_for_the_negative_rows(self):
        # The same layouts without the hostile part are accepted, so the rows above test it.
        from ctl_extra import with_member

        with_member(self, b'"note": "fine"')
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsupported_layout"))
        self.cleanup()
        self.setUp()
        with_member(self, b'"note": [[{"k": ["x \\\\q"]}]]')
        self.assertEqual(self.inspect()["reason"], "unsupported_layout")

    def test_invalid_input_never_qualifies_a_committed_journal(self):
        for name, build, _, _, _ in EXTRA_ROWS[:4]:
            with self.subTest(row=name):
                self.setUp()
                try:
                    build(self)
                    self.assertNotEqual(self.inspect()["reason"], "unsupported_layout")
                finally:
                    self.cleanup()

    def test_mode_000_lock_is_classified_without_opening_it(self):
        build = {row[0]: row[1] for row in EXTRA_ROWS}["lock with mode 000"]
        build(self)
        self.assertEqual(self.inspect()["status"], "ready")

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFOs exist on Unix only")
    def test_no_entry_is_opened_to_classify_it(self):
        # Opening a FIFO without a writer would block a reader that opens it.
        for name in ("pipe", ".harflow-store.lock", "workflows", "store.lock"):
            with self.subTest(name=name):
                self.setUp()
                try:
                    os.makedirs(self.state)
                    os.mkfifo(os.path.join(self.state, name))
                    started = time.monotonic()
                    result = self.inspect(timeout=10)
                    self.assertLess(time.monotonic() - started, 10)
                    self.assertEqual(result["status"], "incompatible")
                finally:
                    self.cleanup()


@unittest.skipIf(os.name == "nt", "peak resident size comes from the POSIX resource module")
class MemoryBoundTests(CtlCase):
    def peak(self):
        environment = {k: v for k, v in os.environ.items() if k not in SCRIPTED_ENV}
        environment.update(HARFLOW_STATE_ROOT=self.state, HOME=self.home)
        result = subprocess.run(
            [sys.executable, "-I", "-c", MEASURE, str(binary())], env=environment,
            capture_output=True, encoding="utf-8", timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        code, peak = result.stdout.split()
        self.assertEqual(code, "0")
        # macOS reports bytes, Linux kilobytes.
        return int(peak) * (1 if sys.platform == "darwin" else 1024)

    def test_many_members_under_a_large_key_stay_within_a_small_multiple_of_the_file(self):
        build = {row[0]: row[1] for row in EXTRA_ROWS}["many members under a 1 MiB key"]
        build(self)
        size = os.path.getsize(os.path.join(self.state, ".harflow-recovery.json"))
        self.assertGreater(size, 1 << 20)
        self.assertLess(size, 16 * 1024 * 1024)
        started = time.monotonic()
        peak = self.peak()
        self.assertLess(time.monotonic() - started, 30)
        # Copying the key per member would need about 10 GiB; the real use is tens of MiB.
        self.assertLess(peak, MEMORY_LIMIT, peak)

    def test_baseline_the_measurement_sees_ordinary_use(self):
        os.makedirs(self.state)
        peak = self.peak()
        self.assertGreater(peak, 100_000)
        self.assertLess(peak, MEMORY_LIMIT)


class DescriptorSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = DESCRIPTOR.read_text(encoding="utf-8")
        cls.code = "\n".join(line for line in cls.source.splitlines() if not line.lstrip().startswith("//"))

    def test_enumeration_has_no_path_based_listing(self):
        self.assertNotIn("read_dir", self.code)
        self.assertNotIn("std::fs::", self.code)
        self.assertRegex(self.code, r"openat\(")
        self.assertRegex(self.code, r"Dir::from_fd")

    def test_names_become_text_only_after_the_last_filesystem_call_that_uses_them(self):
        lossy = [m.start() for m in re.finditer(r"to_string_lossy", self.code)]
        self.assertEqual(len(lossy), 1)
        entries = self.code[self.code.index("pub(super) fn entries"):]
        entries = entries[:entries.index("\n    }\n") + 1]
        position = entries.index("to_string_lossy")
        for call in ("self.kind_of(&name)", "self.is_empty_directory(&name)", "self.names("):
            self.assertIn(call, entries)
            self.assertGreater(position, entries.rindex(call), call)
        self.assertGreater(self.code.index("to_string_lossy"), self.code.index("pub(super) fn entries"))

    # Non-UTF-8 names cannot be created on APFS (EILSEQ), so that case is not exercised
    # here; the source checks above are the only evidence available on this host.


if __name__ == "__main__":
    unittest.main()

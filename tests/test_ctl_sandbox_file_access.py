"""File-access evidence for the store-backed reads, from deny-default `sandbox-exec` profiles.

Each profile is a template under tests/sandbox, filled with the harflow-ctl path and the resolved
state root. A read that succeeds with the same output under a profile that allows only a literal
file set shows that file set is an upper bound of what the read touches. Limits: no row or byte
evidence, `/bin/ps` runs outside the sandbox (it is setuid), and `sandbox-exec` and SBPL are
deprecated and undocumented, so this holds for the macOS versions it passes on. On macOS arm64 a
missing `sandbox-exec` fails the tests. Violation-log counts are not asserted: denied attempts that
the engine tolerates (for example write attempts on the WAL index) are expected.
"""

import hashlib
import json
import os
import subprocess
from pathlib import Path

import ctl_support
import seed_support as seeds
from reads_support import ReadsCase
from seed_support import A, D

SANDBOX_EXEC = "/usr/bin/sandbox-exec"
TEMPLATES = Path(__file__).resolve().parent / "sandbox"
WIDE = ("2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z")
MARKER = {"HARFLOW_TASK_ID": "x"}
STORE_READS = {
    "list": ("list",),
    "status": ("status", A),
    "status-batch": ("status-batch", "--task-ids", A),
    "task-list-page": ("task-list-page",),
    "task-events": ("task-events", A),
    "task-prompt": ("task-prompt", D),
    "task-artifacts": ("task-artifacts", A),
    "recovery-activity": ("recovery-activity", A),
    "interaction-events": ("interaction-events",),
    "statistics-page": ("statistics-page", "--since", WIDE[0], "--before", WIDE[1]),
}
UNREADABLE = "State root cannot be read"


def without_observed_at(value):
    """Drop `observed_at` at every level: it is the one field that differs between runs."""
    if isinstance(value, dict):
        return {k: without_observed_at(v) for k, v in value.items() if k != "observed_at"}
    if isinstance(value, list):
        return [without_observed_at(v) for v in value]
    return value


class SandboxCase(ReadsCase):
    def setUp(self):
        super().setUp()
        self.assertTrue(
            os.access(SANDBOX_EXEC, os.X_OK),
            "sandbox-exec is required on macOS arm64 for the file-access evidence",
        )
        self.seed("seed_world", {"world": seeds.world() | {"events": {A: seeds.a_events()}}})
        self.root = os.path.realpath(self.state)
        self.ctl = os.path.realpath(str(ctl_support.binary()))

    def profile(self, name):
        text = (TEMPLATES / f"{name}.sbpl.tmpl").read_text(encoding="utf-8")
        text = text.replace("@BINARY@", self.ctl).replace("@ROOT@", self.root)
        self.assertNotIn("@BINARY@", text)
        self.assertNotIn("@ROOT@", text)
        path = os.path.join(self.base, f"{name}.sb")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        return path

    def run_ctl(self, args, profile=None, marker=False):
        environment = {"PATH": "/usr/bin:/bin", "HARFLOW_STATE_ROOT": self.root}
        if marker:
            environment.update(MARKER)
        command = [self.ctl, *args, "--json"]
        if profile is not None:
            command = [SANDBOX_EXEC, "-f", self.profile(profile), *command]
        done = subprocess.run(
            command, cwd=self.work, env=environment, stdin=subprocess.DEVNULL,
            capture_output=True, encoding="utf-8", timeout=seeds.TIMEOUT,
        )
        stdout = json.loads(done.stdout) if done.stdout.strip() else None
        return done.returncode, done.stderr, without_observed_at(stdout)


class AuthorisedReadTests(SandboxCase):
    def test_every_store_read_is_identical_under_the_narrow_file_set(self):
        for name, args in STORE_READS.items():
            with self.subTest(command=name):
                plain = self.run_ctl(args)
                self.assertEqual(plain[0], 0, plain)
                self.assertEqual(self.run_ctl(args, "authorised"), plain)

    def test_every_store_read_succeeds_when_only_the_lock_file_is_writable(self):
        for name, args in STORE_READS.items():
            with self.subTest(command=name):
                plain = self.run_ctl(args)
                self.assertEqual(plain[0], 0, plain)
                self.assertEqual(self.run_ctl(args, "read_tree_lock_write"), plain)

    def test_unsandboxed_reads_leave_every_root_file_but_the_shm_unchanged(self):
        before = self.digests()
        for args in STORE_READS.values():
            self.assertEqual(self.run_ctl(args)[0], 0)
        self.assertEqual(self.digests(), before)

    def digests(self):
        result = {}
        for name in sorted(os.listdir(self.root)):
            if name.endswith("-shm"):
                continue
            with open(os.path.join(self.root, name), "rb") as handle:
                result[name] = hashlib.sha256(handle.read()).hexdigest()
        return result


class MarkerRefusalTests(SandboxCase):
    def test_a_marker_refusal_needs_only_root_classification_files(self):
        for name, args in STORE_READS.items():
            if name == "task-list-page":
                continue
            with self.subTest(command=name):
                plain = self.run_ctl(args, marker=True)
                self.assertEqual(plain[0], 1, plain)
                self.assertEqual(self.run_ctl(args, "marker_refused", marker=True), plain)

    def test_the_plain_page_with_a_marker_needs_the_full_file_set(self):
        args = STORE_READS["task-list-page"]
        plain = self.run_ctl(args, marker=True)
        self.assertEqual(plain[0], 1, plain)
        self.assertNotEqual(self.run_ctl(args, "marker_refused", marker=True), plain)
        self.assertEqual(self.run_ctl(args, "authorised", marker=True), plain)


class NoRootAccessTests(SandboxCase):
    def test_every_store_read_fails_closed_without_access_to_the_root(self):
        for name, args in STORE_READS.items():
            with self.subTest(command=name):
                code, stderr, document = self.run_ctl(args, "no_root")
                self.assertEqual(code, 1, (stderr, document))
                self.assertEqual(document["error"]["code"], "state_incompatible")
                self.assertEqual(document["error"]["message"], UNREADABLE)
                self.assertEqual(stderr, "harflow-ctl: error: %s\n" % UNREADABLE)

    def test_commands_that_do_not_need_the_root_still_answer_without_it(self):
        version = self.run_ctl(("version-info",))
        self.assertEqual(version[0], 0, version)
        self.assertEqual(self.run_ctl(("version-info",), "no_root"), version)
        code, stderr, document = self.run_ctl(("state-inspect",), "no_root")
        self.assertEqual((code, stderr), (0, ""), document)
        self.assertEqual(document["result"]["status"], "incompatible")
        self.assertEqual(document["result"]["message"], UNREADABLE)
        self.assertNotIn("error", document)

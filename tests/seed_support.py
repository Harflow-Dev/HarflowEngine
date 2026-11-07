"""Shared helpers for the hf-fixture-seed black-box tests.

The oracle is Python's own json, os, hashlib and sqlite3 plus `ps` and `sysctl`: nothing is
shared with the Rust codecs. The binary is built the way ctl_support.binary() builds
harflow-ctl. Every call runs with the environment exactly `PATH=/usr/bin:/bin`, as the
conformance runner invokes an adapter. The tool's production lease works on macOS arm64 only.
"""

import hashlib
import json
import os
import platform
import shutil
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENVIRONMENT = {"PATH": "/usr/bin:/bin"}
TIMEOUT = 60
WAL = "store.sqlite3-wal"
SUPPORTED = platform.system() == "Darwin" and platform.machine() == "arm64"
_binary = None

A = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
B = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
C = "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
D = "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
E = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee"
LONG_PROMPT = "".join(f"line {n:04d} é spaced   text\n" for n in range(120))
LEGACY = (
    "legacy_alive_with_markers", "legacy_markers_missing", "legacy_markers_seen",
    "legacy_markers_unseen", "legacy_no_markers",
)


def binary():
    """Build once, then return the path of the debug hf-fixture-seed executable."""
    global _binary
    if _binary is None:
        environment = os.environ.copy()
        cargo_home = environment.get("CARGO_HOME")
        if cargo_home:
            environment["PATH"] = os.pathsep.join(
                [str(Path(cargo_home) / "bin"), environment.get("PATH", "")]
            )
        cargo = shutil.which("cargo", path=environment["PATH"])
        if cargo is None:
            raise RuntimeError("cargo is required for the hf-fixture-seed tests")
        subprocess.run(
            [cargo, "build", "--locked", "-p", "hf-fixture-seed", "--bin", "hf-fixture-seed"],
            cwd=ROOT, env=environment, check=True,
        )
        metadata = subprocess.check_output(
            [cargo, "metadata", "--locked", "--format-version", "1", "--no-deps"],
            cwd=ROOT, env=environment, encoding="utf-8",
        )
        target = Path(json.loads(metadata)["target_directory"])
        name = "hf-fixture-seed.exe" if os.name == "nt" else "hf-fixture-seed"
        _binary = target / "debug" / name
    return _binary


def task(task_id, **fields):
    """A complete add_task spec: the values of the conformance world's `task()`."""
    spec = {
        "task_id": task_id, "backend": "claude", "repo_path": "/workspace/repo",
        "freedom": "write_in_repo", "session_id": "session-" + task_id[:8],
        "status": "completed", "exit_code": 0, "started_at": "2026-01-10T10:00:00+00:00",
        "finished_at": "2026-01-10T10:05:00+00:00", "prompt": "Summarize the repo.",
        "output_drained": True,
    }
    spec.update(fields)
    return spec


def event(seq, kind, observed_at, **fields):
    return {"seq": seq, "kind": kind, "observed_at": observed_at, "fields": fields}


def a_events():
    stamp = "2026-01-10T10:0%d:00+00:00"
    return [
        event(0, "task_started", stamp % 0, backend="claude"),
        event(1, "assistant_delta", stamp % 1, text="Sum"),
        event(2, "assistant_delta", stamp % 1, text="marizing"),
        event(3, "assistant_text", stamp % 2, text="Summarizing the repository."),
        event(4, "tool_call", stamp % 3, call_id="call-1", name="Read", input={"path": "README.md"}),
        event(5, "tool_result", stamp % 3, call_id="call-1", is_error=False, output="Readme body"),
        event(6, "usage", stamp % 4, input_tokens=120, output_tokens=30),
        event(7, "task_finished", stamp % 5, status="completed", exit_code=0),
    ]


def b_events():
    stamp = "2026-01-11T10:0%d:00+00:00"
    return [
        event(0, "task_started", stamp % 0, backend="claude"),
        event(1, "assistant_text", stamp % 1, text="Starting."),
        event(2, "notice", stamp % 2, text="stream ended"),
    ]


def world():
    return {
        "tasks": [
            task(A, title="First task", group="g1", model="model-a"),
            task(B, started_at="2026-01-11T10:00:00+00:00", finished_at="2026-01-11T10:03:00+00:00",
                 status="failed", exit_code=1, prompt="Fix the bug.", title="Second task"),
            task(C, started_at="2026-01-12T10:00:00+00:00", finished_at="2026-01-12T10:02:00+00:00",
                 continuation_of=A, prompt="Continue.", title="Third task"),
            task(D, started_at="2026-01-13T10:00:00+00:00", finished_at="2026-01-13T10:02:00+00:00",
                 prompt=LONG_PROMPT, prompt_store="sidecar_verified"),
            task(E, started_at="2026-01-14T10:00:00+00:00", finished_at="2026-01-14T10:02:00+00:00",
                 prompt=LONG_PROMPT, prompt_store="sidecar_missing"),
        ],
        "events": {A: a_events(), B: b_events()},
    }


def sha256(text):
    return hashlib.sha256(text.encode("utf-8") if isinstance(text, str) else text).hexdigest()


@unittest.skipUnless(SUPPORTED, "the fixture seed tool runs on macOS arm64 only")
class SeedCase(unittest.TestCase):
    """Per-test scratch area: base/state is the (not yet created) state root."""

    def setUp(self):
        # A symlinked ancestor such as /var/folders is refused, so resolve the base.
        self.base = os.path.realpath(tempfile.mkdtemp(prefix="hf-fixture-seed-test-"))
        self.addCleanup(shutil.rmtree, self.base, ignore_errors=True)
        self.state = os.path.join(self.base, "state")
        self.succeeded = set()

    def call(self, op, arguments=None, root=None, stdin=None):
        data = stdin if stdin is not None else json.dumps(arguments or {})
        return subprocess.run(
            [str(binary()), op, "--root", root or self.state], input=data.encode("utf-8"),
            capture_output=True, env=dict(ENVIRONMENT), timeout=TIMEOUT,
        )

    def ok(self, op, arguments=None, expect="null\n", **options):
        """The call succeeds: exit 0, nothing on stderr, exactly one line on stdout."""
        done = self.call(op, arguments, **options)
        self.assertEqual(done.returncode, 0, done.stderr.decode(errors="replace"))
        self.assertEqual(done.stderr, b"")
        self.succeeded.add(op)
        text = done.stdout.decode("utf-8")
        if expect is not None:
            self.assertEqual(text, expect)
        self.assertTrue(text.endswith("\n") and text.count("\n") == 1, repr(text))
        return json.loads(text)

    def refused(self, op, arguments=None, message=None, **options):
        """The call fails: exit 1, nothing on stdout, a prefixed message on stderr."""
        done = self.call(op, arguments, **options)
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertEqual(done.stdout, b"")
        text = done.stderr.decode("utf-8")
        self.assertTrue(text.startswith("hf-fixture-seed: ") and text.endswith("\n"), repr(text))
        if message is not None:
            self.assertIn(message, text)
        return text

    def digest(self, root=None):
        return self.ok("authoritative_digest", root=root, expect=None)

    def wal_path(self):
        return os.path.join(self.state, WAL)

    def assert_wal(self):
        info = os.lstat(self.wal_path())
        self.assertTrue(os.path.isfile(self.wal_path()) and not os.path.islink(self.wal_path()), info)

    def initialized(self, *tasks):
        self.ok("initialize_empty")
        for spec in tasks:
            self.ok("add_task", {"spec": spec}, expect="{}\n")

    def database(self):
        """A connection to a private copy of the root: reading never touches the original."""
        copy = os.path.join(self.base, "copy-%d" % len(os.listdir(self.base)))
        shutil.copytree(self.state, copy)
        connection = sqlite3.connect(os.path.join(copy, "store.sqlite3"))
        self.addCleanup(connection.close)
        return connection

    def rows(self, sql, *params):
        return self.database().execute(sql, params).fetchall()

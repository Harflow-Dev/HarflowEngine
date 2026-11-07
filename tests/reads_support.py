"""Shared helpers for the store-backed read black-box tests.

The oracle is Python's own json, os, subprocess and sqlite3: nothing is shared with the Rust
codecs. Roots are built with hf-fixture-seed and read with the built harflow-ctl. The test
process is the parent of every harflow-ctl it starts, so a record on `os.getpid()` is a record
on an ancestor of the engine.
"""

import json
import os
import subprocess
import unittest

import seed_support as seeds
from ctl_support import CtlCase, one_document, run

UNSET = ("PB_TASK_ID", "PB_DEPTH")
MARKER_KEYS = ("HARFLOW_TASK_ID", "PB_TASK_ID", "HARFLOW_DEPTH", "PB_DEPTH")
ENVELOPE = {"product", "format", "v", "product_version"}
AGENT_TEXT = "is an agent"
UNDECIDABLE_TEXT = "could not establish"
MISSING = "99999999-9999-4999-8999-999999999999"
STAMP = "2026-01-%02dT10:00:00+00:00"


def day(number):
    return STAMP % number


@unittest.skipUnless(seeds.SUPPORTED, "the store-backed reads run on macOS arm64 only")
class ReadsCase(CtlCase):
    """A scratch root seeded through the fixture port and read through harflow-ctl."""

    def seed(self, op, arguments=None, root=None):
        done = subprocess.run(
            [str(seeds.binary()), op, "--root", root or self.state],
            input=json.dumps(arguments or {}).encode("utf-8"),
            capture_output=True, env=dict(seeds.ENVIRONMENT), timeout=seeds.TIMEOUT,
        )
        self.assertEqual(done.returncode, 0, done.stderr.decode(errors="replace"))
        return json.loads(done.stdout.decode("utf-8"))

    def init(self, *tasks, root=None):
        self.seed("initialize_empty", root=root)
        for spec in tasks:
            self.seed("add_task", {"spec": spec}, root=root)

    def add(self, task_id, root=None, **fields):
        self.seed("add_task", {"spec": seeds.task(task_id, **fields)}, root=root)

    def live(self, task_id, pid=None, identity="alive", status="running", root=None):
        reply = self.seed("add_live_caller", {
            "task_id": task_id, "pid": os.getpid() if pid is None else pid,
            "identity": identity, "status": status, "marker": None,
        }, root=root)
        self.assertEqual(reply, {"supported": True})

    def sql(self, statement, params=()):
        """Edit the root's database directly; a store root must keep its write-ahead log."""
        import sqlite3

        path = os.path.join(self.state, "store.sqlite3")
        db = sqlite3.connect(path)
        try:
            db.execute(statement, params)
            db.commit()
        finally:
            db.close()
        wal = path + "-wal"
        if not os.path.exists(wal):
            with open(wal, "xb"):
                pass

    def digest(self, root=None):
        return self.seed("authoritative_digest", root=root)

    def sleeper(self):
        """A child process that stays alive until the test ends."""
        child = subprocess.Popen(["/bin/sleep", "600"], stdin=subprocess.DEVNULL)
        self.addCleanup(self.reap, child)
        return child

    @staticmethod
    def reap(child):
        if child.poll() is None:
            child.kill()
        child.wait()

    def read(self, *args, env=None, root="default", **options):
        root = self.state if root == "default" else root
        options.setdefault("home", self.home)
        options.setdefault("cwd", self.work)
        return run(list(args), root=root, env=env, unset=UNSET, **options)

    def good(self, *args, **options):
        """A successful JSON read: exit 0, nothing on stderr, one envelope document."""
        done = self.read(*args, "--json", **options)
        self.assertEqual((done.returncode, done.stderr), (0, ""), done.stdout)
        document = one_document(done)
        self.assertTrue(ENVELOPE <= set(document), document)
        return document

    def bad(self, *args, code=None, **options):
        """A runtime refusal: exit 1, one stderr line, the error document on stdout."""
        done = self.read(*args, "--json", **options)
        self.assertEqual(done.returncode, 1, (done.stdout, done.stderr))
        document = one_document(done)
        error = document["error"]
        self.assertEqual(set(document), ENVELOPE | {"error"})
        self.assertEqual(done.stderr, "harflow-ctl: error: %s\n" % error["message"])
        if code is not None:
            self.assertEqual(error["code"], code, error)
        return error

    def ids(self, items):
        return [item["task_id"] for item in items]

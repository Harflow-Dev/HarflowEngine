"""No history read changes the store's raw bytes, the files beside it or the artifact targets."""

import os

import seed_support as seeds
from ctl_support import snapshot
from history_support import ArtifactCase, event, item
from seed_support import A, B


def tree(path):
    """Every entry under `path` with its mode, size and mtime; SQLite keeps touching the shared
    memory file of a store it reads, so only that file's mtime is left out."""
    return {key: value[:2] if key.endswith("-shm") else value for key, value in snapshot(path).items()}


class NoSideEffectTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.seed("remove_root")
        self.seed("seed_world", {"world": {"tasks": [
            seeds.task(A, real_workspace=True, prompt=seeds.LONG_PROMPT, prompt_store="sidecar_verified", title="Named"),
            seeds.task(B, started_at="2026-01-11T10:00:00+00:00", prompt_store="sidecar_missing"),
        ], "events": {A: seeds.a_events(), B: [event(n, "usage") for n in range(5)]}}})
        self.seed("append_partial_line", {"task_id": B})
        self.seed("append_malformed_line", {"task_id": A})
        self.register(*(item(f"x{n}", f"f{n}.txt") for n in range(6)))
        out = os.path.join(self.repo(A), "out")
        os.remove(os.path.join(out, "f1.txt"))
        os.mkfifo(os.path.join(out, "f1.txt"))
        os.remove(os.path.join(out, "f2.txt"))
        os.symlink(os.path.join(out, "f0.txt"), os.path.join(out, "f2.txt"))
        os.remove(os.path.join(out, "f3.txt"))
        os.chmod(os.path.join(out, "f4.txt"), 0)
        self.seed("change_artifact_target", {"task_id": A, "name": "f5.txt"})

    def families(self):
        return (
            ("task-events", A), ("task-events", A, "--all-kinds", "--limit", "2"), ("task-events", A, "--from", "oldest"),
            ("task-events", A[:8], "--kinds", "notice"), ("task-events", B), ("task-events", B, "--cursor", "garbage"),
            ("task-prompt", A), ("task-prompt", B), ("task-prompt", A, "--max-bytes", "10"),
            ("task-artifacts", A), ("task-artifacts", A, "--limit", "2"), ("task-artifacts", B),
            ("task-artifacts", "bad/id"), ("task-artifacts", A, "--cursor", "garbage"),
            ("status", A, "--brief"), ("status", B, "--brief"), ("status", A), ("status-batch", "--task-ids", f"{A},{B}"),
            ("list",), ("task-list-page",), ("task-list-page", "--predecessor-task-id", A),
        )

    def test_no_read_changes_the_raw_digest_the_file_list_or_the_targets(self):
        digest = self.digest()
        names = sorted(os.listdir(self.state))
        before = tree(self.base)
        for args in self.families():
            with self.subTest(args=args):
                self.read(*args, "--json")
                self.assertEqual(self.digest(), digest)
                self.assertEqual(sorted(os.listdir(self.state)), names)
        self.assertEqual(tree(self.base), before)

    def test_refused_reads_change_nothing_either(self):
        digest = self.digest()
        before = tree(self.base)
        for key in ("HARFLOW_TASK_ID", "HARFLOW_ROOT_TASK_ID"):
            for args in self.families():
                self.read(*args, "--json", env={key: "x"})
        self.assertEqual(self.digest(), digest)
        self.assertEqual(tree(self.base), before)
        self.live("0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f")
        digest = self.digest()
        before = tree(self.base)
        for args in self.families():
            self.read(*args, "--json")
        self.assertEqual(self.digest(), digest)
        self.assertEqual(tree(self.base), before)

    def test_the_targets_keep_their_kinds_after_every_read(self):
        out = os.path.join(self.repo(A), "out")
        for args in self.families():
            self.read(*args, "--json")
        self.assertTrue(os.path.islink(os.path.join(out, "f2.txt")))
        self.assertEqual(sorted(os.listdir(out)), ["f0.txt", "f1.txt", "f2.txt", "f4.txt", "f5.txt"])
        self.assertEqual(oct(os.stat(os.path.join(out, "f4.txt")).st_mode & 0o777), "0o0")
        os.chmod(os.path.join(out, "f4.txt"), 0o600)

    def test_reading_a_root_with_no_store_creates_nothing(self):
        absent = os.path.join(self.base, "nothing")
        for args in (("task-events", A), ("task-prompt", A), ("task-artifacts", A)):
            self.read(*args, "--json", root=absent)
        self.assertFalse(os.path.lexists(absent))

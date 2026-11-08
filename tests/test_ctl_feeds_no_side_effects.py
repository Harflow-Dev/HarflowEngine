"""`recovery-activity`, `interaction-events` and `statistics-page` change nothing: not the store's
raw bytes, not the files beside it, and a missing root stays missing."""

import os

import seed_support as seeds
from ctl_support import snapshot
from feeds_support import WIDE, FeedsCase, stats_cursor
from seed_support import A, B

LIVE = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f"


def tree(path):
    """Every entry under `path`; SQLite keeps touching the shared-memory file, so only its mtime is left out."""
    return {key: value[:2] if key.endswith("-shm") else value for key, value in snapshot(path).items()}


class NoSideEffectTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world() | {"events": {A: seeds.a_events()}}})

    def families(self):
        window = ("--since", WIDE[0], "--before", WIDE[1])
        return (
            ("recovery-activity", A), ("recovery-activity", A[:8]), ("recovery-activity", "bad/id"),
            ("recovery-activity", A, "--limit", "0", "--cursor", "x"),
            ("interaction-events",), ("interaction-events", "--limit", "2"), ("interaction-events", "--cursor", "zzz"),
            ("interaction-events", "--limit", "0"),
            ("statistics-page", *window), ("statistics-page", *window, "--limit", "2"),
            ("statistics-page", *window, "--cursor", stats_cursor(self.state, WIDE, "g", 0)),
            ("statistics-page", "--since", "x", "--before", "y"),
        )

    def check(self, env=None):
        digest = self.digest()
        names = sorted(os.listdir(self.state))
        before = tree(self.base)
        for args in self.families():
            with self.subTest(args=args, env=env):
                self.read(*args, "--json", env=env)
                self.read(*args, env=env)
                self.assertEqual(self.digest(), digest)
                self.assertEqual(sorted(os.listdir(self.state)), names)
        self.assertEqual(tree(self.base), before)

    def test_no_read_changes_the_raw_digest_or_the_file_list(self):
        self.check()

    def test_refused_reads_change_nothing_either(self):
        self.check(env={"HARFLOW_TASK_ID": "x"})
        self.check(env={"HARFLOW_TASK_ID": ""})
        self.live(LIVE)
        self.check()

    def test_a_damaged_root_and_a_foreign_root_are_left_as_they_are(self):
        self.seed("damage_task", {"task_id": B})
        self.check()
        self.seed("remove_root")
        self.seed("make_foreign_root")
        before = tree(self.base)
        for args in self.families():
            self.read(*args, "--json")
        self.assertEqual(tree(self.base), before)

    def test_reading_a_root_with_no_store_creates_nothing(self):
        absent = os.path.join(self.base, "nothing")
        for args in self.families():
            self.read(*args, "--json", root=absent)
            self.read(*args, root=absent)
        self.assertFalse(os.path.lexists(absent))
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        for args in self.families():
            self.read(*args, "--json", root=empty)
        self.assertEqual(os.listdir(empty), [])

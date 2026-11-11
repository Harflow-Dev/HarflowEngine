"""The interaction-events feed against the reference engine, at realistic sizes.

The same world is written twice: with the reference engine's own store writer into a Python
state layout, and into a Rust store through the fixture port. The pinned `interaction_events.page`
(HarflowMCP commit 0ac7e3c, extracted with `git archive`, python3 3.13.5, run in a subprocess) is
walked at several limits, and so is `harflow-ctl interaction-events`; every page must be equal
(cursors are compared as decoded objects, because the engine writes its cursor canonically).

The one difference this cannot show is the 8 MiB metadata budget: the reference engine counts the
bytes of the task files, the Rust store counts the stored header and status blobs, so the page
boundary under that budget can differ. It is unreachable within 25 sources at these sizes (25
records of about 2 KiB), and the black-box test in test_ctl_interaction_events.py reaches it on
purpose. The comparison needs a sibling HarflowMCP checkout and is skipped only when the checkout is absent (and fails when it is absent under
HARFLOW_REQUIRE_MCP_REFERENCE=1); a checkout that cannot give the pinned commit, or a reference that
cannot be imported, fails the test.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

import seed_support as seeds
from feeds_support import FeedsCase, decode_cursor

COMMIT = "0ac7e3c"
LIMITS = (1, 2, 7, 25, 100)
HERE = os.path.dirname(os.path.abspath(__file__))
MCP = os.path.join(os.path.dirname(os.path.dirname(HERE)), "HarflowMCP")
TITLES = (
    "Fix the flaky retry test in the scheduler", "Summarize café menu \U0001F600 changes", "", None,
    "x" * 300, "Refactor `store` module: split \"reads\" from \\writes\\", "\U0001F600" * 205, "Release notes for v1.2",
)
STATUSES = (("completed", 0), ("failed", 2), ("running", None), ("cancelled", 130), ("timed_out", 124), ("completed", None))


def world(count=40):
    tasks = []
    for number in range(count):
        status, code = STATUSES[number % len(STATUSES)]
        start = "2026-01-%02dT%02d:%02d:00+00:00" % (1 + number % 28, number % 24, (number * 7) % 60)
        end = "2026-01-%02dT%02d:%02d:30+00:00" % (1 + number % 28, number % 24, (number * 7) % 60) if status != "running" and number % 6 != 5 else None
        tasks.append({
            "task_id": ("t%02d" % number) if number % 3 else "%08x-0000-4000-8000-%012x" % (number * 977, number),
            "backend": "claude", "session_id": "session-%d" % number, "repo_path": "/work/repositories/project-%d/" % number + "dir/" * 20,
            "started_at": start, "finished_at": end, "status": status, "exit_code": code, "title": TITLES[number % len(TITLES)],
            "model": "claude-model-%d" % number, "markers": ["marker-%d-%s" % (n, "m" * 40) for n in range(12)],
        })
    return tasks


def extract():
    if not os.path.isdir(os.path.join(MCP, ".git")) and not os.path.isfile(os.path.join(MCP, ".git")):
        if os.environ.get("HARFLOW_REQUIRE_MCP_REFERENCE") == "1":
            raise AssertionError("HARFLOW_REQUIRE_MCP_REFERENCE=1 and there is no HarflowMCP checkout at " + MCP)
        raise unittest.SkipTest("no sibling HarflowMCP checkout: the reference comparison is skipped")
    target = tempfile.mkdtemp(prefix="harflow-reference-")
    archive = subprocess.run(["git", "-C", MCP, "archive", COMMIT, "src"], capture_output=True)
    if archive.returncode != 0:
        raise AssertionError("HarflowMCP exists but %s cannot be archived: %s" % (COMMIT, archive.stderr.decode()[-300:]))
    subprocess.run(["tar", "-x", "-C", target], input=archive.stdout, check=True)
    stubs = os.path.join(target, "stubs")
    os.mkdir(stubs)
    with open(os.path.join(stubs, "tomlkit.py"), "w") as stub:
        stub.write("# the reference imports tomlkit at load time; the feed never uses it\n")
    return target


class ReferenceComparisonTests(FeedsCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.sources = extract()

    def reference(self, tasks):
        root = os.path.join(self.base, "reference")
        env = dict(os.environ, HARFLOW_STATE_ROOT=root, PYTHONPATH=os.pathsep.join([
            os.path.join(self.sources, "stubs"), os.path.join(self.sources, "src")]))
        done = subprocess.run([sys.executable, os.path.join(HERE, "feeds_reference.py")],
                              input=json.dumps({"tasks": tasks, "limits": LIMITS}), capture_output=True,
                              text=True, env={k: v for k, v in env.items() if k != "PYTHONHOME"}, timeout=120)
        self.assertEqual(done.returncode, 0, done.stderr[-2000:])
        return json.loads(done.stdout)["walks"]

    def seeded(self, tasks):
        specs = []
        for spec in tasks:
            specs.append(seeds.task(spec["task_id"], backend=spec["backend"], repo_path=spec["repo_path"],
                                    session_id=spec["session_id"], status=spec["status"], exit_code=spec["exit_code"],
                                    started_at=spec["started_at"], finished_at=spec["finished_at"],
                                    title=spec["title"], model=spec["model"]))
        self.world(*specs)

    @staticmethod
    def shape(page):
        """The page with its cursor decoded: the engine writes the cursor canonically."""
        cursor = page["next_cursor"]
        return dict(page, next_cursor=None if cursor is None else decode_cursor(cursor))

    def assert_same(self, tasks):
        walks = self.reference(tasks)
        self.seeded(tasks)
        for limit in LIMITS:
            with self.subTest(limit=limit):
                ours = [self.shape(page) for page in self.feed_walk("--limit", str(limit))]
                theirs = [self.shape(page) for page in walks[str(limit)]]
                self.assertEqual(ours, theirs)
                self.assertTrue(ours[-1]["cycle_complete"])
        return walks

    def test_forty_mixed_tasks_give_identical_pages_at_every_limit(self):
        walks = self.assert_same(world(40))
        self.assertGreater(sum(len(page["events"]) for page in walks["100"]), 10)
        self.assertGreater(len(walks["100"]), 1, "the 25-source window was crossed")

    def test_only_settled_tasks_and_a_few_sources_give_identical_pages(self):
        tasks = [spec for spec in world(30) if spec["status"] == "completed" and spec["exit_code"] == 0]
        self.assert_same(tasks)

    def test_a_world_beyond_two_pages_of_sources_is_identical(self):
        walks = self.assert_same(world(60))
        self.assertGreaterEqual(len(walks["100"]), 3)

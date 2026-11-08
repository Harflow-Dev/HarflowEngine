"""Black-box tests of the caller gate of `interaction-events` and `statistics-page`, and the order of
the gates of the three feed commands on roots that are foreign, missing or damaged.

Codes and messages are those of ctl.py, takeover.py and statistics.py of HarflowMCP commit a426c5a
(decisions C1 sections 6 and 7): statistics refuses with agent_caller or caller_undecidable and a
fixed prefix, interaction-events with `unauthorized` and one message whatever the cause.
"""

import json
import os

import seed_support as seeds
from feeds_support import WIDE, FeedsCase
from reads_support import AGENT_TEXT, MARKER_KEYS, UNDECIDABLE_TEXT
from seed_support import A, B

LIVE = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f"
PREFIX = "Statistics require a verified direct caller: "
IE_TEXT = "Interaction events require a direct caller"
STATS = ("statistics-page", "--since", WIDE[0], "--before", WIDE[1])
FEED = ("interaction-events",)
RECOVERY = ("recovery-activity", A)


class GateTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(B, started_at="2026-01-11T10:00:00+00:00",
                                            finished_at="2026-01-11T10:05:00+00:00"))

    def refused(self, command, code, text=None, **options):
        error = self.bad(*command, code=code, **options)
        if text is not None:
            self.assertIn(text, error["message"])
        return error

    def test_an_agent_ancestor_is_refused_by_both_reads(self):
        self.live(LIVE)
        error = self.refused(STATS, "agent_caller", AGENT_TEXT)
        self.assertTrue(error["message"].startswith(PREFIX))
        error = self.refused(FEED, "unauthorized")
        self.assertEqual(error["message"], IE_TEXT)

    def test_an_identity_that_cannot_be_decided_is_caller_undecidable_but_unauthorized_for_events(self):
        self.live(LIVE, identity="unknown")
        error = self.refused(STATS, "caller_undecidable", UNDECIDABLE_TEXT)
        self.assertTrue(error["message"].startswith(PREFIX))
        self.assertEqual(self.refused(FEED, "unauthorized")["message"], IE_TEXT)

    def test_a_dead_or_unseen_identity_admits_the_caller(self):
        for identity in ("reused",):
            with self.subTest(identity=identity):
                self.live(LIVE, identity=identity)
                self.assertEqual(self.stats()["state"], "ready")
                self.assertEqual(len(self.feed()["events"]), 2)

    def test_a_marker_refuses_by_presence_even_when_empty(self):
        for key in MARKER_KEYS:
            for value in ("x", ""):
                with self.subTest(key=key, value=value):
                    self.refused(STATS, "agent_caller", env={key: value})
                    self.refused(FEED, "unauthorized", env={key: value})

    def test_the_root_task_marker_is_not_one_of_the_four(self):
        for value in ("x", ""):
            self.assertEqual(self.stats(env={"HARFLOW_ROOT_TASK_ID": value})["state"], "ready")
            self.feed(env={"HARFLOW_ROOT_TASK_ID": value})

    def test_the_ten_marker_sets_of_the_conformance_case_refuse_both_reads(self):
        sets = [{key: value} for key in MARKER_KEYS for value in ("1", "")] + [
            {"HARFLOW_TASK_ID": "", "PB_TASK_ID": ""}, {"HARFLOW_DEPTH": "0", "PB_DEPTH": ""}]
        for env in sets:
            with self.subTest(env=env):
                self.refused(STATS, "agent_caller", env=env)
                self.refused(FEED, "unauthorized", env=env)

    def test_a_refusal_is_identical_whatever_the_query(self):
        self.live(LIVE)
        bodies = set()
        for args in (STATS, STATS + ("--limit", "0"), STATS[:1] + ("--since", "x", "--before", "y")):
            done = self.read(*args, "--json")
            self.assertEqual(done.returncode, 1)
            bodies.add(done.stdout)
        self.assertEqual(len(bodies), 1)
        feed_bodies = {self.read("interaction-events", *extra, "--json").stdout
                       for extra in ((), ("--limit", "0"), ("--cursor", "zzz"))}
        self.assertEqual(len(feed_bodies), 1)

    def test_a_usage_error_comes_before_authority(self):
        done = self.read("statistics-page", "--before", WIDE[1], "--json", env={"HARFLOW_TASK_ID": "x"})
        self.assertEqual(done.returncode, 2)
        done = self.read("interaction-events", "--limit", "x", "--json", env={"HARFLOW_TASK_ID": "x"})
        self.assertEqual(done.returncode, 2)

    def test_authority_comes_before_the_parameters(self):
        self.refused(STATS + ("--limit", "0"), "agent_caller", env={"HARFLOW_TASK_ID": "x"})
        self.refused(("statistics-page", "--since", "nonsense", "--before", "nonsense"), "agent_caller",
                     env={"HARFLOW_TASK_ID": "x"})
        self.refused(FEED + ("--limit", "0"), "unauthorized", env={"HARFLOW_TASK_ID": "x"})
        self.refused(FEED + ("--cursor", "zzz"), "unauthorized", env={"HARFLOW_TASK_ID": "x"})

    def test_a_damaged_root_is_undecidable_and_unauthorized(self):
        self.seed("damage_task", {"task_id": B})
        error = self.refused(STATS, "caller_undecidable", UNDECIDABLE_TEXT)
        self.assertTrue(error["message"].startswith(PREFIX))
        self.assertEqual(self.refused(FEED, "unauthorized")["message"], IE_TEXT)
        self.refused(RECOVERY, "workflow_read_refused")


class ForeignRootTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))
        self.seed("remove_root")
        self.seed("make_foreign_root")

    def test_without_a_marker_every_command_reports_the_incompatible_root(self):
        for command in (STATS, FEED, RECOVERY):
            with self.subTest(command=command[0]):
                self.bad(*command, code="state_incompatible")

    def test_with_a_marker_statistics_is_refused_first_and_the_other_two_see_the_root_first(self):
        env = {"HARFLOW_TASK_ID": "x"}
        self.bad(*STATS, code="agent_caller", env=env)
        self.bad(*FEED, code="state_incompatible", env=env)
        self.bad(*RECOVERY, code="state_incompatible", env=env)
        env = {"HARFLOW_TASK_ID": ""}
        self.bad(*STATS, code="agent_caller", env=env)
        self.bad(*FEED, code="state_incompatible", env=env)
        self.bad("recovery-activity", "bad/id", code="state_incompatible", env=env)


class MissingRootTests(FeedsCase):
    def test_every_command_leaves_a_missing_or_empty_root_untouched(self):
        absent = os.path.join(self.base, "nothing")
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        for root in (absent, empty):
            for command in (STATS, FEED):
                with self.subTest(root=os.path.basename(root), command=command[0]):
                    done = self.read(*command, "--json", root=root)
                    self.assertEqual(done.returncode, 0, done.stdout)
                    self.assertTrue(json.loads(done.stdout)["result"])
            self.bad(*RECOVERY, code="unknown_task", root=root)
        self.assertFalse(os.path.lexists(absent))
        self.assertEqual(os.listdir(empty), [])

    def test_a_missing_root_answers_the_ready_empty_statistics_page(self):
        absent = os.path.join(self.base, "nothing")
        page = self.stats(root=absent)
        self.assertEqual((page["state"], page["items"], page["history_complete"]), ("ready", [], True))
        self.assertEqual(page["accounting_coverage"]["eligible_executions"], 0)
        self.bad(*STATS, code="agent_caller", root=absent, env={"HARFLOW_TASK_ID": ""})
        self.stats_error(code="invalid_params", window=("x", "y"), root=absent)

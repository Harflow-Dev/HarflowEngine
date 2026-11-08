"""Black-box tests of `interaction-events`: which tasks yield an event, the event rows, the
25-source cycle, resumption by cursor state, gaps and the 8 MiB metadata budget.

Literals: epoch seconds and event keys come from `datetime.fromisoformat(...).timestamp()` and
`f'{timestamp:024.6f}'` of python3 3.13.5, the calls interaction_events.py (HarflowMCP a426c5a)
makes.
"""

import json

import seed_support as seeds
from reads_support import ENVELOPE
from feeds_support import FeedsCase, decode_cursor, settled, stamp
from feeds_cursors import compact, encode
from seed_support import A, B, C, D, E

OCCURRED = {A: 1768039500.0, B: 1768125780.0, C: 1768212120.0, D: 1768298520.0, E: 1768384920.0}
TITLES = {A: "First task", B: "Second task", C: "Third task", D: D, E: E}
STATES = {A: "completed", B: "failed", C: "completed", D: "completed", E: "completed"}
KEY = "00000001768039500.000000"
ROW_KEYS = {"event_id", "interaction_id", "kind", "state", "task_id", "workflow_run_id",
            "source_workflow_run_id", "title", "occurred_at"}


def state(**changes):
    value = {"v": 1, "phase": "task", "after": "", "source": "", "event": ""}
    value.update(changes)
    return encode(compact(value))


class FeedContentTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def test_every_settled_task_gives_one_event_in_id_order(self):
        page = self.feed()
        self.assertEqual(set(page), {"events", "next_cursor", "cycle_complete", "history_incomplete", "gaps"})
        self.assertEqual((page["next_cursor"], page["cycle_complete"]), (None, True))
        self.assertEqual((page["history_incomplete"], page["gaps"]), (False, []))
        self.assertEqual([row["task_id"] for row in page["events"]], [A, B, C, D, E])
        for row in page["events"]:
            task = row["task_id"]
            with self.subTest(task=task):
                self.assertEqual(set(row), ROW_KEYS)
                expected = {
                    "event_id": "task:%s:%s" % (task, STATES[task]), "interaction_id": task, "kind": "task",
                    "state": STATES[task], "task_id": task, "workflow_run_id": None,
                    "source_workflow_run_id": None, "title": TITLES[task], "occurred_at": OCCURRED[task],
                }
                self.assertEqual(row, expected)
                self.assertIs(type(row["occurred_at"]), float)

    def test_the_documents_are_stamped_and_printed_the_way_each_mode_prints(self):
        done = self.read("interaction-events", "--json")
        document = json.loads(done.stdout)
        self.assertEqual(set(document), ENVELOPE | {"result"})
        self.assertEqual(done.stdout.count("\n"), 1)
        bare = self.read("interaction-events")
        self.assertEqual((bare.returncode, bare.stderr), (0, ""))
        # Without --json the bare result is indented by two, as json.dumps(result, indent=2) prints it.
        self.assertEqual(bare.stdout, json.dumps(document["result"], indent=2, ensure_ascii=False) + "\n")
        self.assertNotIn("product", json.loads(bare.stdout))

    def test_a_limit_walk_equals_the_unpaged_feed_and_names_its_resume_point(self):
        whole = self.feed()["events"]
        pages = self.feed_walk("--limit", "2")
        self.assertEqual([len(page["events"]) for page in pages], [2, 2, 1])
        self.assertEqual([page["cycle_complete"] for page in pages], [False, False, True])
        self.assertEqual([event for page in pages for event in page["events"]], whole)
        self.assertEqual(decode_cursor(pages[0]["next_cursor"]),
                         {"v": 1, "phase": "task", "after": B, "source": C, "event": ""})
        self.assertEqual(decode_cursor(pages[1]["next_cursor"]),
                         {"v": 1, "phase": "task", "after": D, "source": E, "event": ""})

    def test_a_page_exactly_at_the_limit_completes_when_no_source_is_left(self):
        page = self.feed("--limit", "5")
        self.assertEqual((len(page["events"]), page["cycle_complete"], page["next_cursor"]), (5, True, None))
        page = self.feed("--limit", "4")
        self.assertEqual((len(page["events"]), page["cycle_complete"]), (4, False))
        self.assertEqual(decode_cursor(page["next_cursor"])["source"], E)

    def test_limits_one_and_one_hundred_are_accepted(self):
        for limit in ("1", "100"):
            self.assertTrue(self.feed("--limit", limit)["events"])

    def test_a_resume_by_event_skips_the_events_up_to_that_key(self):
        event_id = "%s:1:task:%s:completed" % (KEY, A)
        for label, event, expected in (
            ("the key of the event", event_id, [B, C, D, E]),
            ("a key just below it", event_id[:-1], [A, B, C, D, E]),
            ("a key above it", "9" + event_id, [B, C, D, E]),
        ):
            with self.subTest(case=label):
                page = self.feed("--cursor", state(source=A, event=event))
                self.assertEqual([row["task_id"] for row in page["events"]], expected)
                self.assertTrue(page["cycle_complete"])

    def test_a_cursor_source_below_after_is_read_and_the_walk_resumes_after_it(self):
        page = self.feed("--cursor", state(after=C, source=A))
        self.assertEqual([row["task_id"] for row in page["events"]], [A, B, C, D, E])

    def test_a_cursor_after_resumes_after_that_id(self):
        page = self.feed("--cursor", state(after=C))
        self.assertEqual([row["task_id"] for row in page["events"]], [D, E])

    def test_an_absent_task_source_is_a_gap_and_the_walk_goes_on_after_it(self):
        missing = "bbbbbbbb-cccc-4000-8000-000000000000"
        page = self.feed("--cursor", state(source=missing))
        self.assertEqual(page["gaps"], [{"source": missing, "reason": "source_unavailable"}])
        self.assertTrue(page["history_incomplete"])
        self.assertEqual([row["task_id"] for row in page["events"]], [C, D, E])
        self.assertTrue(page["cycle_complete"])

    def test_a_workflow_source_is_a_gap_and_the_task_phase_follows(self):
        text = encode(compact({"v": 1, "phase": "workflow", "after": "", "source": "run1", "event": ""}))
        page = self.feed("--cursor", text)
        self.assertEqual(page["gaps"], [{"source": "run1", "reason": "source_unavailable"}])
        self.assertTrue(page["history_incomplete"])
        self.assertEqual([row["task_id"] for row in page["events"]], [A, B, C, D, E])

    def test_a_workflow_cursor_after_a_source_goes_straight_to_the_task_phase(self):
        text = encode(compact({"v": 1, "phase": "workflow", "after": "run9", "source": "", "event": ""}))
        page = self.feed("--cursor", text)
        self.assertEqual((page["gaps"], page["history_incomplete"]), ([], False))
        self.assertEqual(len(page["events"]), 5)


class FeedCandidateTests(FeedsCase):
    def test_only_completed_and_failed_tasks_with_an_exit_code_give_events(self):
        self.world(
            settled("t1", 0, status="completed", exit_code=0), settled("t2", 1, status="failed", exit_code=3),
            settled("t3", 2, status="timed_out", exit_code=None), settled("t4", 3, status="cancelled", exit_code=None),
            settled("t5", 4, status="cancelled", exit_code=0), settled("t6", 5, status="timed_out", exit_code=1),
            settled("t7", 6, status="completed", exit_code=None), settled("t8", 7, status="failed", exit_code=None),
            seeds.task("t9", status="running", exit_code=None, finished_at=None),
            seeds.task("ta", status="running", exit_code=0),
        )
        page = self.feed()
        self.assertEqual([(row["task_id"], row["state"]) for row in page["events"]],
                         [("t1", "completed"), ("t2", "failed")])
        self.assertEqual((page["history_incomplete"], page["cycle_complete"]), (False, True))

    def test_the_time_is_the_finish_or_else_the_start(self):
        self.world(
            seeds.task("t1", started_at=stamp(0), finished_at=None),
            seeds.task("t2", started_at=stamp(0), finished_at=stamp(5, second=30)),
            seeds.task("t3", started_at="2026-01-10T10:00:00.123456+00:00", finished_at="2026-01-10T11:00:00.5+01:00"),
        )
        got = {row["task_id"]: row["occurred_at"] for row in self.feed()["events"]}
        self.assertEqual(got, {"t1": 1768039200.0, "t2": 1768039530.0, "t3": 1768039200.5})

    def test_a_title_falls_back_to_the_id_when_missing_or_empty_and_is_cut_to_200_code_points(self):
        long = "\U0001f600" * 201
        self.world(
            settled("t1", 0, title=None), settled("t2", 1, title=""), settled("t3", 2, title="café \"q\" \\ 中"),
            settled("t4", 3, title=long), settled("t5", 4, title="x" * 200), settled("t6", 5, title="y" * 201),
            settled("t7", 6, title=" "),
        )
        got = {row["task_id"]: row["title"] for row in self.feed()["events"]}
        self.assertEqual(got["t1"], "t1")
        self.assertEqual(got["t2"], "t2")
        self.assertEqual(got["t3"], "café \"q\" \\ 中")
        self.assertEqual(got["t4"], "\U0001f600" * 200)
        self.assertEqual(got["t5"], "x" * 200)
        self.assertEqual(got["t6"], "y" * 200)
        self.assertEqual(got["t7"], " ")

    def test_sources_follow_the_byte_order_of_their_ids(self):
        ids = ["alpha", "a_b", "a1", "a-b", "Zeta", "9lives", "ZZ"]
        self.world(*[settled(task, index) for index, task in enumerate(ids)])
        self.assertEqual([row["task_id"] for row in self.feed()["events"]], sorted(ids))

    def test_ordering_is_by_id_not_by_time(self):
        self.world(settled("a1", 30, day=12), settled("b1", 0, day=10), settled("c1", 10, day=11))
        page = self.feed()
        self.assertEqual([row["task_id"] for row in page["events"]], ["a1", "b1", "c1"])
        self.assertEqual([row["occurred_at"] for row in page["events"]], [1768214100.0, 1768039500.0, 1768126500.0])


class FeedEventKeyTests(FeedsCase):
    """Event keys are `f'{timestamp:024.6f}:{phase}:{event_id}'`, compared as text."""

    KEYS = (
        ("2026-01-10T10:05:00+00:00", "00000001768039500.000000"),
        ("1969-12-31T23:59:58.500000+00:00", "-0000000000000001.500000"),
        ("1970-01-01T00:00:00+00:00", "00000000000000000.000000"),
        ("1970-01-01T00:00:01.007812+00:00", "00000000000000001.007812"),
        ("9999-12-31T23:59:59.999999+00:00", "00000253402300800.000000"),
        ("0001-01-01T00:00:00+00:00", "-0000062135596800.000000"),
        ("2026-01-10T10:05:00.123456+00:00", "00000001768039500.123456"),
        ("2200-06-15T12:00:00.654321+00:00", "00000007272417600.654321"),
        ("1970-01-01T00:00:00.000001+00:00", "00000000000000000.000001"),
        ("1969-12-31T23:59:59.999999+00:00", "-0000000000000000.000001"),
    )

    def test_a_cursor_holding_the_reference_key_skips_exactly_that_event(self):
        for index, (finished, key) in enumerate(self.KEYS):
            task = "k%02d" % index
            with self.subTest(finished=finished):
                self.world(seeds.task(task, started_at=finished, finished_at=finished))
                event_id = "%s:1:task:%s:completed" % (key, task)
                skipped = self.feed("--cursor", state(source=task, event=event_id))
                self.assertEqual(skipped["events"], [])
                kept = self.feed("--cursor", state(source=task, event=event_id[:-1]))
                self.assertEqual([row["task_id"] for row in kept["events"]], [task])
                self.seed("remove_root")
        self.assertEqual(KEY, self.KEYS[0][1])


class FeedBoundaryTests(FeedsCase):
    def tasks(self, count, running_every=3):
        found = []
        for index in range(count):
            running = running_every and index % running_every == 0
            found.append(settled("t%02d" % index, index % 50, status="running" if running else "completed",
                                 exit_code=None if running else 0))
        return found

    def test_a_page_reads_at_most_25_sources_running_ones_included(self):
        self.world(*self.tasks(30))
        first = self.feed()
        expected = ["t%02d" % n for n in range(25) if n % 3]
        self.assertEqual([row["task_id"] for row in first["events"]], expected)
        self.assertFalse(first["cycle_complete"])
        self.assertEqual(decode_cursor(first["next_cursor"]),
                         {"v": 1, "phase": "task", "after": "t24", "source": "", "event": ""})
        second = self.feed("--cursor", first["next_cursor"])
        self.assertEqual([row["task_id"] for row in second["events"]], ["t%02d" % n for n in range(25, 30) if n % 3])
        self.assertEqual((second["cycle_complete"], second["next_cursor"]), (True, None))

    def test_25_sources_end_the_page_with_a_cursor_and_26_leave_one_for_the_next(self):
        for count, tail in ((25, []), (26, ["t25"])):
            with self.subTest(count=count):
                self.world(*self.tasks(count, running_every=0))
                first = self.feed()
                self.assertEqual(len(first["events"]), 25)
                self.assertFalse(first["cycle_complete"])
                second = self.feed("--cursor", first["next_cursor"])
                self.assertEqual([row["task_id"] for row in second["events"]], tail)
                self.assertTrue(second["cycle_complete"])
                self.seed("remove_root")

    def test_24_sources_complete_in_one_page(self):
        self.world(*self.tasks(24, running_every=0))
        page = self.feed()
        self.assertEqual((len(page["events"]), page["cycle_complete"]), (24, True))

    def test_a_limit_inside_the_25_source_window_resumes_in_the_source(self):
        self.world(*self.tasks(30, running_every=0))
        pages = self.feed_walk("--limit", "7")
        self.assertEqual([len(page["events"]) for page in pages], [7, 7, 7, 7, 2])
        ids = [row["task_id"] for page in pages for row in page["events"]]
        self.assertEqual(ids, ["t%02d" % n for n in range(30)])


class FeedMetadataBudgetTests(FeedsCase):
    """The budget counts the stored header and status blob bytes of the sources of a page."""

    LIMIT = 8 * 1024 * 1024

    def sized(self, first, second):
        self.world(settled("m1", 0), settled("m2", 1), settled("m3", 2))
        for task, size in (("m1", first), ("m2", second), ("m3", 1)):
            self.sql("UPDATE task_headers SET header=zeroblob(?), status=zeroblob(0) WHERE id=?", (size, task))
        stored = self.reading("SELECT id, length(header)+length(status) FROM task_headers ORDER BY id", ())
        self.assertEqual([size for _, size in stored], [first, second, 1])

    def test_sources_up_to_exactly_the_budget_are_read_and_the_next_one_waits(self):
        self.sized(self.LIMIT // 2, self.LIMIT // 2)
        page = self.feed()
        self.assertEqual([row["task_id"] for row in page["events"]], ["m1", "m2"])
        self.assertFalse(page["cycle_complete"])
        self.assertEqual(decode_cursor(page["next_cursor"])["after"], "m2")
        rest = self.feed("--cursor", page["next_cursor"])
        self.assertEqual([row["task_id"] for row in rest["events"]], ["m3"])
        self.assertTrue(rest["cycle_complete"])

    def test_one_byte_over_the_budget_stops_before_that_source(self):
        self.sized(self.LIMIT // 2, self.LIMIT // 2 + 1)
        page = self.feed()
        self.assertEqual([row["task_id"] for row in page["events"]], ["m1"])
        self.assertEqual(decode_cursor(page["next_cursor"])["after"], "m1")
        rest = self.feed("--cursor", page["next_cursor"])
        self.assertEqual([row["task_id"] for row in rest["events"]], ["m2", "m3"])
        self.assertTrue(rest["cycle_complete"])

    def test_a_first_source_over_the_budget_is_still_read(self):
        self.sized(self.LIMIT + 1, 1)
        page = self.feed()
        self.assertEqual([row["task_id"] for row in page["events"]], ["m1"])
        self.assertEqual(decode_cursor(page["next_cursor"])["after"], "m1")

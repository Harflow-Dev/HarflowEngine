"""Black-box tests of `statistics-page` items, windows, ordering, retention and float spelling.

Literals: the items of tasks A, B and C are the output of `project()` in statistics.py of HarflowMCP
commit a426c5a (python3 3.13.5) over the same facts; float spellings are `repr()` of the stdlib
`datetime.fromisoformat(text).timestamp()` values and of their differences; the retention table is
`_retention()` of the same module under each value of HARFLOW_RETENTION_DAYS.
"""

import json

import seed_support as seeds
from feeds_support import EMPTY_GENERATION, ITEM_KEYS, WIDE, FeedsCase, micros, settled
from reads_support import ENVELOPE
from seed_support import A, B, C, D, E

ACCOUNTING = {
    "additive": False, "scope": "unknown", "state": "unavailable", "total_tokens": None, "version": 1,
    "categories": {"cache_creation_tokens": None, "cache_read_tokens": None, "input_tokens": None, "output_tokens": None},
    "diagnostics": ["legacy_missing_accounting"], "qualified_wait_intervals": [],
}
NULLS = ("activation_id", "cost_provenance", "cost_usd", "node_id", "observed_model", "observed_model_provenance",
         "owner_task_id", "parent_task_id", "root_workflow_run_id", "workflow_id", "workflow_name",
         "workflow_revision", "workflow_run_id")
ITEM_A = {
    "accounting": dict(ACCOUNTING, contribution_id="task:" + A), "chain_root_task_id": None,
    "cohort_started_at": 1768039200.0, "continuation_of_task_id": None, "cost_additive": False,
    "duration_additive": True, "elapsed_seconds": 300.0, "execution_kind": "initial",
    "execution_seconds": 300.0, "finished_at": 1768039500.0, "id": "execution:" + A, "kind": "execution",
    "repo_path": "/workspace/repo", "requested_harness": "claude", "requested_model": "model-a",
    "started_at": 1768039200.0, "status": "completed", "task_id": A,
    "time_qualification": "elapsed_may_include_waits", "title": "First task", "usage_additive": False,
    "wait_seconds": 0, **{key: None for key in NULLS},
}
ITEM_B = dict(
    ITEM_A, accounting=dict(ACCOUNTING, contribution_id="task:" + B), cohort_started_at=1768125600.0,
    elapsed_seconds=180.0, execution_seconds=180.0, finished_at=1768125780.0, id="execution:" + B,
    requested_model=None, started_at=1768125600.0, status="failed", task_id=B, title="Second task",
)
ITEM_C = dict(
    ITEM_A, accounting=dict(ACCOUNTING, contribution_id="task:" + C), chain_root_task_id=A,
    cohort_started_at=1768212000.0, continuation_of_task_id=A, elapsed_seconds=120.0,
    execution_kind="continuation", execution_seconds=120.0, finished_at=1768212120.0, id="execution:" + C,
    requested_model=None, started_at=1768212000.0, task_id=C, title="Third task",
)
RETENTION = (
    (None, {"state": "days", "days": 30}), ("", {"state": "unknown", "days": None}),
    ("  ", {"state": "unknown", "days": None}), (" 7 ", {"state": "days", "days": 7}),
    ("1_0", {"state": "days", "days": 10}), ("-1", {"state": "unknown", "days": None}),
    ("-0", {"state": "disabled", "days": None}), ("0", {"state": "disabled", "days": None}),
    ("+0", {"state": "disabled", "days": None}), ("x", {"state": "unknown", "days": None}),
    ("1__0", {"state": "unknown", "days": None}), ("_1", {"state": "unknown", "days": None}),
    ("1_", {"state": "unknown", "days": None}), ("7.0", {"state": "unknown", "days": None}),
    ("0x10", {"state": "unknown", "days": None}), ("1e3", {"state": "unknown", "days": None}),
    ("+7", {"state": "days", "days": 7}), ("007", {"state": "days", "days": 7}),
    ("1_000", {"state": "days", "days": 1000}), ("0_0", {"state": "disabled", "days": None}),
    ("\t9\n", {"state": "days", "days": 9}),
    ("9223372036854775807", {"state": "days", "days": 9223372036854775807}),
    ("18446744073709551615", {"state": "days", "days": 18446744073709551615}),
)
# Beyond 64 bits Python keeps every digit of the integer.
HUGE_RETENTION = (
    ("18446744073709551616", {"state": "days", "days": 18446744073709551616}),
    ("99999999999999999999999", {"state": "days", "days": 99999999999999999999999}),
)
# Python's int() also reads other scripts' digits.
UNICODE_RETENTION = (
    ("\u0661\u0662", {"state": "days", "days": 12}), ("\u0660", {"state": "disabled", "days": None}),
    ("\uff17", {"state": "days", "days": 7}),
)
# (task, start, end, started_at, finished_at, elapsed_seconds) as repr() of the reference engine's floats.
FLOATS = (
    ("f1", "1970-01-01T00:00:00.000001+00:00", "1970-01-01T00:00:00.000002+00:00", "1e-06", "2e-06", "1e-06"),
    ("f2", "1970-01-01T00:00:00.000010+00:00", "1970-01-01T00:00:00.000020+00:00", "1e-05", "2e-05", "1e-05"),
    ("f3", "1970-01-01T00:00:00.000015+00:00", "1970-01-01T00:00:00.000030+00:00", "1.5e-05", "3e-05", "1.5e-05"),
    ("f4", "1970-01-01T00:00:00+00:00", "1970-01-01T00:00:00.000100+00:00", "0.0", "0.0001", "0.0001"),
    ("f5", "1970-01-01T00:00:00+00:00", "1970-01-01T00:00:00.000012+00:00", "0.0", "1.2e-05", "1.2e-05"),
    ("f6", "1970-01-01T00:00:00+00:00", "1970-01-01T00:00:00.123456+00:00", "0.0", "0.123456", "0.123456"),
    ("f7", "1970-01-01T00:00:00+00:00", "1970-01-01T00:02:00+00:00", "0.0", "120.0", "120.0"),
    ("f8", "1970-01-01T00:00:05+00:00", "1970-01-01T00:00:05+00:00", "5.0", "5.0", "0.0"),
    ("f9", "1969-12-31T23:59:59.999999+00:00", "1970-01-01T00:00:00+00:00", "-1e-06", "0.0", "1e-06"),
    ("fb", "2026-01-10T10:00:00.000001+00:00", "2026-01-10T10:00:00.000002+00:00",
     "1768039200.000001", "1768039200.000002", "9.5367431640625e-07"),
    ("fc", "2200-06-15T12:00:00.654321+00:00", "2200-06-15T12:30:00.654321+00:00",
     "7272417600.654321", "7272419400.654321", "1800.0"),
    ("fd", "9999-12-31T12:00:00.000001+00:00", "9999-12-31T13:00:00.000002+00:00",
     "253402257600.0", "253402261200.0", "3600.0"),
    ("fe", "0001-01-01T00:00:00+01:00", "0001-01-01T00:00:01+01:00", "-62135600400.0", "-62135600399.0", "1.0"),
    ("ff", "2026-01-10T10:00:00+00:00", "2026-01-10T10:00:00.5+00:00", "1768039200.0", "1768039200.5", "0.5"),
)


class ItemTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def test_the_window_lists_every_finished_task_as_one_execution(self):
        page = self.stats()
        self.assertEqual([item["task_id"] for item in page["items"]], [A, B, C, D, E])
        self.assertEqual(page["items"][0], ITEM_A)
        self.assertEqual(page["items"][1], ITEM_B)
        self.assertEqual(page["items"][2], ITEM_C)
        for item in page["items"]:
            self.assertEqual(len(item), ITEM_KEYS)
            self.assertEqual(item["elapsed_seconds"], item["execution_seconds"])
        self.assertEqual([item["elapsed_seconds"] for item in page["items"]], [300.0, 180.0, 120.0, 120.0, 120.0])

    def test_the_document_reports_a_ready_complete_projection(self):
        page = self.stats()
        self.assertEqual(
            {key: page[key] for key in page if key not in ("items", "generation", "snapshot_id", "observed_at")},
            {"projection_version": 1, "state": "ready", "next_cursor": None, "has_more": False,
             "history_complete": True, "ownership_complete": True, "retained_history_only": True,
             "accounting_coverage": {"eligible_executions": 5, "available_executions": 0},
             "cost_coverage": {"eligible_executions": 5, "available_executions": 0},
             "retention": {"state": "days", "days": 30}, "diagnostics": []})
        self.assertRegex(page["generation"], r"^[0-9a-f]{64}$")
        self.assertEqual(page["snapshot_id"], page["generation"])
        self.assertRegex(page["observed_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{6})?\+00:00$")

    def test_waits_are_the_integer_zero_and_durations_floats(self):
        done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--json")
        self.assertIn('"wait_seconds": 0,', done.stdout)
        self.assertNotIn('"wait_seconds": 0.0', done.stdout)
        item = json.loads(done.stdout)["result"]["items"][0]
        self.assertIs(type(item["wait_seconds"]), int)
        for key in ("elapsed_seconds", "execution_seconds", "started_at", "finished_at", "cohort_started_at"):
            self.assertIs(type(item[key]), float, key)
        self.assertIn('"elapsed_seconds": 300.0,', done.stdout)

    def test_without_json_the_bare_result_is_printed_indented(self):
        bare = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1])
        self.assertEqual((bare.returncode, bare.stderr), (0, ""))
        self.assertTrue(bare.stdout.startswith("{\n  "), bare.stdout[:20])
        document = json.loads(bare.stdout)
        self.assertEqual(document["state"], "ready")
        self.assertFalse(ENVELOPE & set(document))
        self.assertEqual([item["task_id"] for item in document["items"]], [A, B, C, D, E])

    def test_since_is_inclusive_and_before_exclusive_by_start(self):
        start_a, start_b = "2026-01-10T10:00:00Z", "2026-01-11T10:00:00Z"
        self.assertEqual([i["task_id"] for i in self.stats(window=(start_a, start_b))["items"]], [A])
        self.assertEqual([i["task_id"] for i in self.stats(window=("2026-01-10T10:00:00.000001Z", start_b))["items"]], [])
        self.assertEqual([i["task_id"] for i in self.stats(window=(start_a, "2026-01-10T10:00:00.000001Z"))["items"]], [A])
        self.assertEqual([i["task_id"] for i in self.stats(window=(start_b, "2026-01-12T10:00:00Z"))["items"]], [B])

    def test_a_window_with_an_offset_is_read_as_that_instant(self):
        window = ("2026-01-10T17:00:00+07:00", "2026-01-11T17:00:00+07:00")
        self.assertEqual([i["task_id"] for i in self.stats(window=window)["items"]], [A])

    def test_a_window_with_no_task_is_ready_and_empty(self):
        page = self.stats(window=("2027-01-01T00:00:00Z", "2027-02-01T00:00:00Z"))
        self.assertEqual((page["state"], page["items"], page["has_more"], page["next_cursor"]), ("ready", [], False, None))
        self.assertEqual((page["history_complete"], page["ownership_complete"]), (True, True))
        self.assertEqual(page["accounting_coverage"], {"eligible_executions": 0, "available_executions": 0})
        self.assertEqual(page["generation"], EMPTY_GENERATION)

    def test_a_limit_walk_is_disjoint_complete_and_under_one_generation(self):
        pages = self.stats_walk("--limit", "2")
        self.assertEqual([len(page["items"]) for page in pages], [2, 2, 1])
        self.assertEqual(len({page["generation"] for page in pages}), 1)
        self.assertEqual([page["has_more"] for page in pages], [True, True, False])
        ids = [item["task_id"] for page in pages for item in page["items"]]
        self.assertEqual(ids, [A, B, C, D, E])
        self.assertEqual([page["accounting_coverage"]["eligible_executions"] for page in pages], [5, 5, 5])
        self.assertEqual(self.stats()["generation"], pages[0]["generation"])

    def test_limits_one_and_one_hundred_are_accepted(self):
        for limit in ("1", "100"):
            self.assertTrue(self.stats("--limit", limit)["items"])


class StatusTests(FeedsCase):
    def test_every_finished_status_is_listed_and_running_is_not(self):
        self.world(
            settled("a1", 0, status="completed"), settled("a2", 1, status="failed", exit_code=2),
            settled("a3", 2, status="cancelled", exit_code=None), settled("a4", 3, status="timed_out", exit_code=None),
            seeds.task("a5", status="running", exit_code=None, finished_at=None),
        )
        page = self.stats()
        self.assertEqual([(i["task_id"], i["status"]) for i in page["items"]],
                         [("a1", "completed"), ("a2", "failed"), ("a3", "cancelled"), ("a4", "timed_out")])
        self.assertEqual(page["accounting_coverage"]["eligible_executions"], 4)

    def test_an_empty_title_is_null_a_missing_model_is_null_and_a_continuation_is_marked(self):
        self.world(settled("p1", 0, title="", model="m"), settled("p2", 10, title=None, continuation_of="p1"))
        first, second = self.stats()["items"]
        self.assertEqual((first["title"], first["requested_model"], first["execution_kind"]), (None, "m", "initial"))
        self.assertEqual((second["title"], second["continuation_of_task_id"], second["chain_root_task_id"]), (None, "p1", "p1"))
        self.assertEqual(second["execution_kind"], "continuation")

    def test_a_running_ancestor_outside_the_window_is_not_an_item(self):
        self.world(seeds.task("r1", status="running", exit_code=None, finished_at=None, started_at="2025-05-01T10:00:00+00:00"),
                   settled("c1", 0, continuation_of="r1"))
        page = self.stats()
        self.assertEqual((page["state"], [i["task_id"] for i in page["items"]]), ("ready", ["c1"]))


class OrderTests(FeedsCase):
    def test_equal_starts_are_ordered_by_id_and_earlier_starts_come_first(self):
        self.world(settled("b", 0), settled("a", 0), settled("c", 0, end=1, day=9))
        self.assertEqual([i["task_id"] for i in self.stats()["items"]], ["c", "a", "b"])

    def test_starts_that_are_equal_as_floats_are_ordered_by_id_even_when_microseconds_differ(self):
        late = "9999-12-31T12:00:00.%06d+00:00"
        self.world(seeds.task("z1", started_at=late % 1, finished_at="9999-12-31T13:00:00+00:00"),
                   seeds.task("a1", started_at=late % 2, finished_at="9999-12-31T13:00:00+00:00"))
        window = ("9999-12-31T00:00:00Z", "9999-12-31T20:00:00Z")
        self.assertLess(micros(late % 1), micros(late % 2))
        pages = self.stats_walk("--limit", "1", window=window)
        self.assertEqual([page["items"][0]["task_id"] for page in pages], ["a1", "z1"])
        self.assertEqual([page["items"][0]["cohort_started_at"] for page in pages], [253402257600.0] * 2)


class FloatSpellingTests(FeedsCase):
    def test_floats_are_written_as_python_repr_writes_them(self):
        self.world(*[seeds.task(name, started_at=start, finished_at=end) for name, start, end, *_ in FLOATS])
        done = self.read("statistics-page", "--since", "0001-01-01T00:00:00+14:00", "--before", "9999-12-31T23:59:59Z",
                         "--limit", "100", "--json")
        self.assertEqual(done.returncode, 0, done.stdout)
        items = {item["task_id"]: item for item in json.loads(done.stdout)["result"]["items"]}
        self.assertEqual(len(items), len(FLOATS))
        for name, _, _, started, finished, elapsed in FLOATS:
            with self.subTest(task=name):
                item = items[name]
                for key, text in (("started_at", started), ("finished_at", finished), ("elapsed_seconds", elapsed)):
                    self.assertEqual(item[key], float(text))
                    self.assertIn('"%s": %s,' % (key, text), self.item_text(done.stdout, name), key)
                self.assertEqual(item["cohort_started_at"], float(started))

    def item_text(self, text, name):
        """The raw text of one item: items are written with sorted keys, so each starts at `accounting`."""
        parts = [part for part in text.split('{"accounting": ')[1:] if '"task_id": "%s"' % name in part]
        self.assertEqual(len(parts), 1, name)
        return parts[0]


class RetentionTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("r1"))

    def retention(self, value):
        env = {} if value is None else {"HARFLOW_RETENTION_DAYS": value}
        done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--json", env=env)
        self.assertEqual(done.returncode, 0, done.stdout)
        return json.loads(done.stdout)["result"]["retention"]

    def test_the_policy_is_read_as_python_reads_the_environment(self):
        for value, expected in RETENTION:
            with self.subTest(value=value):
                self.assertEqual(self.retention(value), expected)

    def test_a_retention_beyond_64_bits_keeps_every_digit(self):
        for value, expected in HUGE_RETENTION:
            with self.subTest(value=value):
                self.assertEqual(self.retention(value), expected)

    def test_other_scripts_digits_are_integers_to_python(self):
        for value, expected in UNICODE_RETENTION:
            with self.subTest(value=value):
                self.assertEqual(self.retention(value), expected)

    def test_the_unavailable_document_reports_the_policy_too(self):
        self.seed("remove_root")
        self.world(settled("c1", continuation_of="ghost"))
        done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--json",
                         env={"HARFLOW_RETENTION_DAYS": "0"})
        result = json.loads(done.stdout)["result"]
        self.assertEqual((result["state"], result["retention"]), ("unavailable", {"state": "disabled", "days": None}))

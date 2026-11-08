"""Black-box tests of the statistics budget in the scan, and of the raw-history cap.

When the budget runs out with no row read the page is `unavailable` ("No source classification
fits the foreground deadline"); with some rows read it is `preparing`, whose diagnostics are the
failures of the rows already read (statistics_index.py of HarflowMCP commit a426c5a, never a
`classification_pending` row); with a cursor either outcome is `snapshot_expired`. More than
100,000 task rows of any validity are `unavailable` with "Raw history name limit exceeded" before
the scan. The budget seam is the debug-only HARFLOW_STATISTICS_BUDGET_MS.
"""

import json

import seed_support as seeds
from feeds_support import STATS_COVERAGE, WIDE, FeedsCase, settled, stats_cursor

NO_PROGRESS = [{"code": "discovery_budget", "message": "No source classification fits the foreground deadline"}]
RAW_CAP = [{"code": "history_limit", "message": "Raw history name limit exceeded"}]
SOURCE_CAP = [{"code": "history_limit", "message": "Statistics source history limit exceeded"}]
BASE_KEYS = {"projection_version", "state", "snapshot_id", "generation", "observed_at", "items", "next_cursor",
             "has_more", "history_complete", "ownership_complete", "accounting_coverage", "cost_coverage",
             "retained_history_only", "retention", "diagnostics"}


class NoProgressTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("b1", 0), settled("b2", 10))

    def budget(self, ms):
        return {"HARFLOW_STATISTICS_BUDGET_MS": str(ms)}

    def test_a_budget_spent_before_any_row_is_unavailable_with_its_own_message(self):
        page = self.stats(env=self.budget(0))
        self.assertEqual((page["state"], page["items"], page["generation"]), ("unavailable", [], None))
        self.assertEqual(page["diagnostics"], NO_PROGRESS)
        self.assertEqual(set(page), BASE_KEYS)
        self.assertEqual((page["history_complete"], page["ownership_complete"], page["has_more"]), (False, False, False))

    def test_a_cursor_walk_is_expired_when_no_row_was_read(self):
        first = self.stats("--limit", "1")
        error = self.stats_error("--cursor", first["next_cursor"], code="snapshot_expired", env=self.budget(0))
        self.assertEqual(error["message"], STATS_COVERAGE)


class PreparingTests(FeedsCase):
    """`preparing` through the debug seam HARFLOW_STATISTICS_EXPIRE_AFTER_ROWS=N: the budget counts
    as expired once N rows have been read, so N=1 and N=50 end inside the first batch of 200 rows and
    N=250 inside the second."""

    BAD = (0, 10, 49, 50, 200, 249, 250, 280)

    def setUp(self):
        super().setUp()
        tasks = [seeds.task("s%03d" % n, status="weird", exit_code=None, finished_at=None) if n in self.BAD
                 else settled("s%03d" % n, n % 50) for n in range(300)]
        self.world(*tasks)

    def expire(self, rows, *args, **options):
        env = {"HARFLOW_STATISTICS_EXPIRE_AFTER_ROWS": str(rows)}
        return self.stats(*args, env=env, **options)

    def failures(self, rows):
        return [{"code": "source_unavailable", "message": "task:s%03d" % n} for n in self.BAD if n < rows]

    def test_no_row_read_is_unavailable_with_its_own_message(self):
        page = self.expire(0)
        self.assertEqual((page["state"], page["diagnostics"]), ("unavailable", NO_PROGRESS))

    def test_a_budget_that_runs_out_after_n_rows_leaves_the_page_preparing_with_their_failures(self):
        for rows in (1, 50, 250):
            with self.subTest(rows=rows):
                page = self.expire(rows)
                self.assertEqual(set(page), BASE_KEYS)
                self.assertEqual((page["state"], page["items"], page["generation"], page["snapshot_id"]),
                                 ("preparing", [], None, None))
                self.assertEqual((page["history_complete"], page["ownership_complete"], page["has_more"],
                                  page["next_cursor"]), (False, False, False, None))
                self.assertEqual(page["accounting_coverage"], {"eligible_executions": 0, "available_executions": 0})
                self.assertEqual(page["diagnostics"], self.failures(rows))
                self.assertNotIn("classification_pending", str(page))

    def test_the_failure_lists_are_exactly_the_first_rows_failures_in_id_order(self):
        self.assertEqual([d["message"] for d in self.expire(1)["diagnostics"]], ["task:s000"])
        self.assertEqual(len(self.expire(50)["diagnostics"]), 3)
        self.assertEqual(len(self.expire(250)["diagnostics"]), 6)

    def test_a_scan_that_has_read_every_row_and_reached_its_end_is_not_cut(self):
        # With N = 300 the budget counts as spent after the last row, before the batch that ends the
        # scan is asked for; one more row, or any number beyond, lets the scan end.
        for rows in (301, 100_000):
            with self.subTest(rows=rows):
                page = self.expire(rows)
                self.assertEqual(page["state"], "unavailable")  # the unknown statuses fail the projection
                self.assertEqual(page["diagnostics"][0], {"code": "source_unavailable", "message": "task:s000"})
        self.assertEqual(self.expire(300)["state"], "preparing")

    def test_a_cursor_walk_is_expired_at_every_n(self):
        self.seed("remove_root")
        self.world(*[settled("v%03d" % n, n % 50) for n in range(300)])
        first = self.stats("--limit", "1")
        for rows in (0, 1, 50, 250):
            with self.subTest(rows=rows):
                error = self.stats_error("--cursor", first["next_cursor"], code="snapshot_expired",
                                         env={"HARFLOW_STATISTICS_EXPIRE_AFTER_ROWS": str(rows)})
                self.assertEqual(error["message"], STATS_COVERAGE)

    def test_a_scan_without_failures_is_preparing_with_no_diagnostics(self):
        self.seed("remove_root")
        self.world(*[settled("v%03d" % n, n % 50) for n in range(300)])
        page = self.expire(10)
        self.assertEqual((page["state"], page["diagnostics"]), ("preparing", []))
        self.assertEqual(page["items"], [])
        self.assertEqual(self.expire(301)["state"], "ready")

    def test_the_failure_list_stops_at_sixteen(self):
        self.seed("remove_root")
        self.world(*[seeds.task("b%02d" % n, status="weird", exit_code=None, finished_at=None) for n in range(20)])
        page = self.expire(20)
        self.assertEqual(page["state"], "preparing")
        self.assertEqual([d["message"] for d in page["diagnostics"]], ["task:b%02d" % n for n in range(16)])

    def test_a_budget_value_that_overflows_the_clock_is_not_a_panic(self):
        self.seed("remove_root")
        self.world(settled("v1", 0))
        for value in ("18446744073709551615", "9223372036854775807", "18446744073709551616"):
            with self.subTest(value=value):
                done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--json",
                                 env={"HARFLOW_STATISTICS_BUDGET_MS": value})
                self.assertEqual(done.returncode, 0, done.stderr[-300:])
                self.assertNotIn("panicked", done.stderr)
                self.assertEqual(json.loads(done.stdout)["result"]["state"], "ready")


class RawCapTests(FeedsCase):
    CAP = 100_000

    def fill(self, count):
        self.init()
        self.sql(
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x<?) "
            "INSERT INTO task_facts(id,valid,backend,repo_path,session_id,status,started_at,started_at_us,output_drained) "
            "SELECT 'r'||x,1,'claude','/r','s','running','2026-01-10T10:00:00+00:00',1768039200000000,1 FROM c", (count,))

    def count(self):
        return self.reading("SELECT count(*) FROM task_facts", ())[0][0]

    def test_100000_rows_reach_the_scan_and_100001_do_not(self):
        self.fill(self.CAP + 1)
        self.assertEqual(self.count(), self.CAP + 1)
        page = self.stats()
        self.assertEqual((page["state"], page["diagnostics"]), ("unavailable", RAW_CAP))
        self.sql("DELETE FROM task_facts WHERE id='r%d'" % (self.CAP + 1))
        self.assertEqual(self.count(), self.CAP)
        page = self.stats()
        self.assertEqual((page["state"], page["diagnostics"]), ("unavailable", SOURCE_CAP))

    def test_a_cursor_walk_over_the_cap_is_expired(self):
        self.fill(self.CAP + 1)
        error = self.stats_error("--cursor", stats_cursor(self.state, WIDE, "g", 0), code="snapshot_expired")
        self.assertEqual(error["message"], STATS_COVERAGE)

    def test_the_cap_is_judged_before_the_scan_even_with_an_expired_budget(self):
        self.fill(self.CAP + 1)
        page = self.stats(env={"HARFLOW_STATISTICS_BUDGET_MS": "0"})
        self.assertEqual(page["diagnostics"], RAW_CAP)

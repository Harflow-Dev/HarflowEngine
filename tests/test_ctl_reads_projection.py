"""Black-box tests of the live projection and the counts of `task-list-page`."""

import seed_support as seeds
from reads_support import ReadsCase, day
from seed_support import A, B

FIELDS = ("status", "process_identity_state", "needs_reconciliation", "observed_exit", "persisted_status")

# (name, how it is seeded, status, identity state, needs reconciliation, persisted, observed)
# Hand-computed from the indexed projection of the reference store: unobserved means status
# running or no exit code; only an alive or undecidable process keeps an unobserved task active.
TABLE = (
    ("observed completed beside an alive process", ("live", "completed_observed", "alive"),
     "completed", "dead", False, "completed", True),
    ("observed completed beside an unknown process", ("live", "completed_observed", "unknown"),
     "completed", "dead", False, "completed", True),
    ("observed failed", ("add", "failed", 1),
     "failed", "dead", False, "failed", True),
    ("running and alive", ("live", "running", "alive"),
     "running", "alive", False, "running", False),
    ("running and dead", ("live", "running", "reused"),
     "unknown", "dead", True, "running", False),
    ("running and undecidable", ("live", "running", "unknown"),
     "running", "uncertain", True, "running", False),
    ("running without a process row", ("add", "running", None),
     "running", "uncertain", True, "running", False),
    ("failed without exit and alive", ("live", "failed_unobserved", "alive"),
     "running", "alive", True, "failed", False),
    ("failed without exit and dead", ("live", "failed_unobserved", "reused"),
     "unknown", "dead", True, "failed", False),
    ("failed without exit and undecidable", ("live", "failed_unobserved", "unknown"),
     "running", "uncertain", True, "failed", False),
    ("completed without exit or process row", ("add", "completed", None),
     "running", "uncertain", True, "completed", False),
)


class ProjectionTests(ReadsCase):
    def seed_row(self, number, how):
        ident = "%08d-0000-4000-8000-000000000000" % number
        stamp = day(number)
        if how[0] == "live":
            self.live(ident, pid=self.alive_pid, identity=how[2], status=how[1])
        else:
            running = how[1] == "running"
            self.add(ident, status=how[1], exit_code=how[2], started_at=stamp, finished_at=None if running else stamp,
                     output_drained=not running)
        return ident

    def test_every_row_of_the_projection_table_in_every_reader(self):
        self.alive_pid = self.sleeper().pid
        self.init()
        seeded = []
        for number, row in enumerate(TABLE, start=1):
            seeded.append((self.seed_row(number, row[1]), row))
        document = self.good("list")
        listed = {task["task_id"]: task for task in document["tasks"]}
        paged = {t["task_id"]: t for t in self.good("task-list-page")["result"]["items"]}
        batch = self.good("status-batch", "--task-ids", ",".join(i for i, _ in seeded))["tasks"]
        batched = {task["task_id"]: task for task in batch}
        for ident, (name, _, status, state, needs, persisted, observed) in seeded:
            wanted = {"status": status, "process_identity_state": state, "needs_reconciliation": needs,
                      "persisted_status": persisted, "observed_exit": observed}
            single = self.good("status", ident)["task"]
            for reader, task in (("status", single), ("list", listed[ident]),
                                 ("page", paged[ident]), ("batch", batched[ident])):
                with self.subTest(row=name, reader=reader):
                    self.assertEqual({key: task[key] for key in FIELDS}, wanted)

    def test_alive_and_running_does_not_need_reconciliation(self):
        self.alive_pid = self.sleeper().pid
        self.init()
        ident = self.seed_row(1, ("live", "running", "alive"))
        self.assertIs(self.good("status", ident)["task"]["needs_reconciliation"], False)
        self.assertIs(self.good("task-list-page")["result"]["items"][0]["needs_reconciliation"], False)

    def test_a_stored_alive_blob_does_not_outlive_the_process(self):
        child = self.sleeper()
        self.init()
        self.live(A, pid=child.pid, identity="alive")
        alive = self.good("task-list-page", "--active-only")["result"]
        self.assertEqual((self.ids(alive["items"]), alive["total_active_root_count"]), ([A], 1))
        child.kill()
        child.wait()
        gone = self.good("task-list-page", "--active-only")["result"]
        self.assertEqual((gone["items"], gone["total_active_root_count"]), ([], 0))
        self.assertEqual(self.good("status", A)["task"]["status"], "unknown")


class CountTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.alive_pid = self.sleeper().pid
        self.init(seeds.task(A, session_id="settled"))

    def proc(self, ident, identity="alive"):
        self.live(ident, pid=self.alive_pid, identity=identity)

    def counts(self, *args):
        result = self.good("task-list-page", *args)["result"]
        return (result["counts_complete"], result["total_active_count"],
                result["total_active_root_count"], result["total_attention_root_count"])

    def test_active_count_follows_the_session_filter_only(self):
        # A live caller records the session "session-" plus the first eight characters of its id.
        self.proc("11111111-0000-4000-8000-000000000001")
        self.proc("11111111-0000-4000-8000-000000000002", identity="reused")
        self.proc("22222222-0000-4000-8000-000000000001")
        self.assertEqual(self.counts(), (True, 2, 2, 0))
        self.assertEqual(self.counts("--session-id", "session-11111111"), (True, 1, 2, 0))
        self.assertEqual(self.counts("--session-id", "session-22222222"), (True, 1, 2, 0))
        self.assertEqual(self.counts("--session-id", "settled"), (True, 0, 2, 0))
        self.assertEqual(self.counts("--session-id", "session-11111111", "--active-only"), (True, 1, 2, 0))
        self.assertEqual(self.counts("--active-only", "--limit", "1"), (True, 2, 2, 0))

    def test_the_predecessor_does_not_scope_the_active_count(self):
        self.proc("11111111-0000-4000-8000-000000000001")
        result = self.good("task-list-page", "--predecessor-task-id", A)["result"]
        self.assertEqual(result["items"], [])
        self.assertEqual((result["total_active_count"], result["total_active_root_count"]), (1, 1))

    def test_an_uncertain_row_in_scope_makes_the_active_count_unknown(self):
        self.proc("11111111-0000-4000-8000-000000000001")
        self.proc("22222222-0000-4000-8000-000000000001", identity="unknown")
        self.assertEqual(self.counts(), (False, None, 2, 0))
        self.assertEqual(self.counts("--session-id", "session-22222222"), (False, None, 2, 0))
        # The uncertain row is outside this session, so the count is known; the root count keeps it.
        self.assertEqual(self.counts("--session-id", "session-11111111"), (True, 1, 2, 0))

    def test_a_row_with_no_process_is_uncertain_and_active(self):
        self.add(B, status="running", exit_code=None, finished_at=None, session_id="solo")
        self.assertEqual(self.counts(), (False, None, 1, 0))
        self.assertEqual(self.counts("--session-id", "other"), (True, 0, 1, 0))

    def test_dead_and_settled_rows_are_not_counted(self):
        self.proc("11111111-0000-4000-8000-000000000001", identity="reused")
        self.assertEqual(self.counts(), (True, 0, 0, 0))


class ActiveOnlyTests(ReadsCase):
    """The filter follows the live projection, not the stored column, across keyset pages."""

    def setUp(self):
        super().setUp()
        self.pid = self.sleeper().pid
        self.init()
        # Live callers share one start instant, so the id breaks the tie: newest first the order
        # is alive, dead, dead, alive, dead, alive, settled. All but the last are stored as
        # running, so the stored column alone would list six of them.
        self.layout = []
        for number, kind in enumerate(("settled", "alive", "dead", "alive", "dead", "dead", "alive"), start=1):
            ident = "%08d-0000-4000-8000-000000000000" % number
            if kind == "settled":
                self.add(ident, started_at=day(number), finished_at=day(number))
            else:
                self.live(ident, pid=self.pid, identity="alive" if kind == "alive" else "reused")
            self.layout.append((ident, kind))
        self.alive = [i for i, k in reversed(self.layout) if k == "alive"]

    def test_pages_hold_only_projected_active_rows_and_cover_them(self):
        seen, cursor = [], None
        for _ in range(5):
            extra = ["--cursor", cursor] if cursor else []
            result = self.good("task-list-page", "--active-only", "--limit", "1", *extra)["result"]
            seen += self.ids(result["items"])
            for item in result["items"]:
                self.assertEqual(item["process_identity_state"], "alive")
            cursor = result["next_cursor"]
            if not result["has_more"]:
                self.assertIsNone(cursor)
                break
            self.assertIsNotNone(cursor)
            self.assertEqual(len(result["items"]), 1)
        self.assertEqual(sorted(seen), sorted(self.alive))
        self.assertEqual(len(seen), len(set(seen)))

    def test_a_page_of_two_skips_dead_rows_across_the_boundary(self):
        first = self.good("task-list-page", "--active-only", "--limit", "2")["result"]
        self.assertEqual(len(first["items"]), 2)
        self.assertIs(first["has_more"], True)
        second = self.good("task-list-page", "--active-only", "--limit", "2",
                           "--cursor", first["next_cursor"])["result"]
        self.assertEqual(len(second["items"]), 1)
        self.assertIs(second["has_more"], False)
        self.assertEqual(sorted(self.ids(first["items"] + second["items"])), sorted(self.alive))

    def test_active_only_with_nothing_active_is_an_empty_final_page(self):
        result = self.good("task-list-page", "--active-only", "--session-id", "session-00000003")["result"]
        self.assertEqual((result["items"], result["has_more"], result["next_cursor"]), ([], False, None))

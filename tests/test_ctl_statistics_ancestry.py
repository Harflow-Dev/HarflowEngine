"""Black-box tests of the `statistics-page` ancestry rules and the unavailable document.

Every verdict and diagnostic list below is the output of `project()` in statistics.py of HarflowMCP
commit a426c5a (python3 3.13.5, sources built in the shape statistics_index.py stores them) over the
same facts: whether ownership is complete, and the diagnostics in the order Python lists them.
"""

import seed_support as seeds
from feeds_support import EMPTY_GENERATION, WIDE, FeedsCase, settled, stamp

MISSING = {"code": "ownership_unavailable", "message": "Task ancestor metadata is unavailable"}
SELF = {"code": "ownership_ambiguous", "message": "Task is its own ancestor"}
CYCLE = {"code": "ownership_ambiguous", "message": "Task ancestry cycle"}
DEPTH = {"code": "ownership_unavailable", "message": "Task ancestry depth exceeds bounded projection"}
NO_FINISH = {"code": "completion_unavailable", "message": "Finished task lacks bounded completion timestamps"}
UNREADABLE = {"code": "source_unavailable", "message": "task:%s"}
OLD = {"started_at": "2025-01-10T10:00:00+00:00", "finished_at": "2025-01-10T10:05:00+00:00"}


class AncestryTests(FeedsCase):
    def unavailable(self, diagnostics):
        page = self.stats()
        self.assertEqual(page["state"], "unavailable", page["diagnostics"])
        self.assertEqual(page["diagnostics"], diagnostics)
        self.assertEqual(
            {key: page[key] for key in page if key != "observed_at" and key != "retention"},
            {"projection_version": 1, "state": "unavailable", "snapshot_id": None, "generation": None, "items": [],
             "next_cursor": None, "has_more": False, "history_complete": False, "ownership_complete": False,
             "accounting_coverage": {"eligible_executions": 0, "available_executions": 0},
             "cost_coverage": {"eligible_executions": 0, "available_executions": 0},
             "retained_history_only": True, "diagnostics": diagnostics})
        return page

    def ready(self, ids):
        pages = self.stats_walk("--limit", "100")
        self.assertEqual({page["state"] for page in pages}, {"ready"}, pages[0]["diagnostics"])
        found = [item["task_id"] for page in pages for item in page["items"]]
        self.assertEqual(found, ids)
        return pages[0]

    def test_a_missing_parent_of_an_included_task_makes_the_page_unavailable(self):
        self.world(settled("c1", continuation_of="ghost"))
        self.unavailable([MISSING])

    def test_a_missing_parent_of_a_task_outside_the_window_does_not(self):
        self.world(seeds.task("c1", continuation_of="ghost", **OLD), settled("c2"))
        self.ready(["c2"])

    def test_a_task_that_continues_itself_is_ambiguous(self):
        self.world(settled("s1", continuation_of="s1"))
        self.unavailable([SELF])

    def test_two_tasks_that_continue_each_other_are_a_cycle(self):
        self.world(settled("c1", continuation_of="c2"), settled("c2", continuation_of="c1"))
        self.unavailable([CYCLE])

    def chain(self, length, parent_of):
        return [settled("t%03d" % n, n % 50, continuation_of=parent_of(n, length)) for n in range(length)]

    def in_order(self, length):
        return ["t%03d" % n for n in sorted(range(length), key=lambda n: (n % 50, "t%03d" % n))]

    def test_a_chain_whose_parents_sort_first_is_memoized_at_any_length(self):
        for length in (129, 130):
            with self.subTest(length=length):
                self.world(*self.chain(length, lambda n, _: ("t%03d" % (n - 1)) if n else None))
                self.ready(self.in_order(length))
                self.seed("remove_root")

    def test_a_chain_whose_parents_sort_last_is_too_deep_at_129_and_fine_at_128(self):
        def parent(n, length):
            return ("t%03d" % (n + 1)) if n < length - 1 else None

        self.world(*self.chain(129, parent))
        self.unavailable([DEPTH])
        self.seed("remove_root")
        self.world(*self.chain(128, parent))
        self.ready(self.in_order(128))

    def test_each_task_with_a_missing_parent_gives_its_own_diagnostic(self):
        self.world(settled("o1", continuation_of="ghost"), settled("o2", continuation_of="ghost"))
        self.unavailable([MISSING, MISSING])

    def test_diagnostics_stop_at_sixteen(self):
        self.world(*[settled("o%02d" % n, n % 50, continuation_of="ghost") for n in range(20)])
        self.unavailable([MISSING] * 16)

    def test_a_parent_with_an_unknown_status_is_absent_and_listed_after_the_child(self):
        self.world(settled("c1", continuation_of="p1"), settled("p1", status="bogus"))
        self.unavailable([MISSING, dict(UNREADABLE, message="task:p1")])

    def test_an_unknown_status_anywhere_makes_the_page_unavailable(self):
        self.world(settled("c1"), seeds.task("zz", status="bogus", exit_code=None, **OLD))
        self.unavailable([dict(UNREADABLE, message="task:zz")])

    def test_a_finished_task_without_a_finish_or_ending_early_is_completion_unavailable(self):
        self.world(seeds.task("n1", started_at=stamp(0), finished_at=None))
        self.unavailable([NO_FINISH])
        self.seed("remove_root")
        self.world(seeds.task("n1", started_at=stamp(0), finished_at=stamp(0, second=0).replace("10:00", "09:00")))
        self.unavailable([NO_FINISH])

    def test_unscoped_diagnostics_come_first_then_ownership_then_unreadable_rows(self):
        self.world(
            seeds.task("n1", started_at=stamp(0), finished_at=None), settled("o1", 1, continuation_of="ghost"),
            seeds.task("n2", started_at=stamp(2), finished_at=None), settled("u1", status="bogus"),
        )
        self.unavailable([NO_FINISH, NO_FINISH, MISSING, dict(UNREADABLE, message="task:u1")])

    def test_a_running_parent_outside_the_window_does_not_block_its_child(self):
        self.world(seeds.task("r1", status="running", exit_code=None, finished_at=None, started_at="2025-05-01T10:00:00+00:00"),
                   settled("c1", continuation_of="r1"))
        self.ready(["c1"])

    def test_an_old_ancestor_with_a_missing_parent_blocks_the_page_of_its_included_child(self):
        self.world(settled("c1", continuation_of="p1"), seeds.task("p1", continuation_of="ghost", **OLD))
        self.unavailable([MISSING])

    def test_an_unavailable_page_has_no_cursor_even_when_one_is_asked_for_by_limit(self):
        self.world(settled("c1", continuation_of="ghost"))
        page = self.stats("--limit", "1")
        self.assertEqual((page["state"], page["next_cursor"], page["generation"]), ("unavailable", None, None))
        self.assertNotEqual(page["generation"], EMPTY_GENERATION)


class AncestryWalkTests(FeedsCase):
    def test_a_walk_whose_projection_turns_incomplete_gets_the_unavailable_document_not_an_error(self):
        self.world(settled("c1"), settled("c2", 10))
        first = self.stats("--limit", "1")
        self.sql("UPDATE task_facts SET continuation_of='ghost', chain_root='ghost' WHERE id='c2'")
        page = self.stats("--cursor", first["next_cursor"])
        self.assertEqual((page["state"], page["diagnostics"], page["items"]), ("unavailable", [MISSING], []))

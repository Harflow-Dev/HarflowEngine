"""Black-box tests of the `statistics-page` generation and of the 10,000-source limit.

The generation is the engine's own digest of the facts a page depends on (the reference engine
digests file witnesses, so only the observable contract is pinned): it changes with every change
to a relevant task and with the store itself, and it stays put for changes nothing on the page
depends on. `EMPTY_GENERATION` is `hashlib.sha256(b'["rs-statistics-1",null,[]]').hexdigest()`.
"""

import hashlib

import seed_support as seeds
from feeds_support import EMPTY_GENERATION, STATS_COVERAGE, STATS_EXPIRED, WIDE, FeedsCase, settled, stats_cursor

OLD = {"started_at": "2025-01-10T10:00:00+00:00", "finished_at": "2025-01-10T10:05:00+00:00"}


class GenerationTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("g1", 0), settled("g2", 10, title="two"))
        self.base_generation = self.generation()

    def generation(self):
        return self.stats()["generation"]

    def test_the_empty_constant_is_the_digest_of_an_empty_relevant_set(self):
        self.assertEqual(hashlib.sha256(b'["rs-statistics-1",null,[]]').hexdigest(), EMPTY_GENERATION)
        self.assertNotEqual(self.base_generation, EMPTY_GENERATION)

    def test_a_reread_and_a_limit_change_keep_the_generation(self):
        self.assertEqual(self.generation(), self.base_generation)
        self.assertEqual(self.stats("--limit", "1")["generation"], self.base_generation)

    def test_changes_that_no_relevant_task_shows_leave_it_alone(self):
        self.add("old1", **OLD)
        self.add("run1", status="running", exit_code=None, finished_at=None, started_at="2026-01-10T11:00:00+00:00")
        self.add("later", started_at="2027-06-01T10:00:00+00:00", finished_at="2027-06-01T10:05:00+00:00")
        self.assertEqual(self.generation(), self.base_generation)
        self.facts("old1", title="changed", status="failed", model="m")
        self.facts("run1", title="changed", repo_path="/elsewhere")
        self.facts("later", backend="codex")
        self.assertEqual(self.generation(), self.base_generation)

    def test_an_added_removed_or_changed_task_of_the_window_changes_it(self):
        seen = {self.base_generation}
        self.add("g3", started_at="2026-01-10T10:20:00+00:00", finished_at="2026-01-10T10:25:00+00:00")
        seen.add(self.generation())
        self.assertEqual(len(seen), 2)
        self.sql("DELETE FROM task_facts WHERE id='g3'")
        self.sql("DELETE FROM task_headers WHERE id='g3'")
        self.assertEqual(self.generation(), self.base_generation)

    def test_each_fact_a_page_shows_changes_it(self):
        changes = (
            ("title", "other title"), ("title", None), ("model", "m2"), ("backend", "codex"), ("repo_path", "/other"),
            ("status", "failed"), ("exit_code", 7), ("exit_code", None),
            ("finished_at_us", 1768039740000000), ("started_at_us", 1768039260000000),
        )
        for column, value in changes:
            with self.subTest(column=column, value=value):
                before = self.generation()
                old = self.reading("SELECT %s FROM task_facts WHERE id='g2'" % column, ())[0][0]
                self.facts("g2", **{column: value})
                self.assertNotEqual(self.generation(), before)
                self.facts("g2", **{column: old})
                self.assertEqual(self.generation(), before)

    def test_an_unreferenced_column_of_a_relevant_task_does_not(self):
        before = self.generation()
        # The text spellings of the times are not read: the instants are the microsecond columns.
        self.facts("g2", session_id="another", output_drained=0, started_at="2026-01-10T10:01:00+00:00",
                   finished_at="2026-01-10T10:09:00+00:00")
        self.assertEqual(self.generation(), before)

    def test_a_finished_ancestor_outside_the_window_counts_but_a_running_one_counts_by_identity_only(self):
        self.add("anc", **OLD)
        self.add("kid", started_at="2026-01-10T12:00:00+00:00", finished_at="2026-01-10T12:05:00+00:00", continuation_of="anc")
        with_ancestor = self.generation()
        self.facts("anc", title="renamed")
        self.assertNotEqual(self.generation(), with_ancestor)
        self.facts("anc", status="running", exit_code=None, finished_at=None, finished_at_us=None)
        running = self.generation()
        self.facts("anc", title="renamed again", repo_path="/x", model="mm", started_at_us=1)
        self.assertEqual(self.generation(), running)
        self.facts("anc", continuation_of="g1", chain_root="g1")
        self.assertNotEqual(self.generation(), running)

    def test_the_same_facts_in_another_store_have_another_generation(self):
        other = self.state + "-2"
        self.world(settled("g1", 0), settled("g2", 10, title="two"), root=other)
        self.assertNotEqual(self.stats(root=other)["generation"], self.base_generation)

    def test_a_cursor_expires_when_a_relevant_fact_changes_and_survives_an_irrelevant_one(self):
        first = self.stats("--limit", "1")
        self.add("old1", **OLD)
        self.assertEqual(self.stats("--limit", "1", "--cursor", first["next_cursor"])["items"][0]["task_id"], "g2")
        self.add("g3", started_at="2026-01-10T10:20:00+00:00", finished_at="2026-01-10T10:25:00+00:00")
        error = self.stats_error("--cursor", first["next_cursor"], code="snapshot_expired")
        self.assertEqual(error["message"], STATS_EXPIRED)


class LimitTests(FeedsCase):
    def many(self, count):
        self.world(*[seeds.task("s%05d" % n, started_at="2026-01-10T10:00:00+00:00",
                                finished_at="2026-01-10T10:01:00+00:00") for n in range(count)])

    def test_exactly_10000_sources_are_projected(self):
        self.many(10_000)
        page = self.stats("--limit", "100")
        self.assertEqual(page["state"], "ready")
        self.assertEqual(page["accounting_coverage"]["eligible_executions"], 10_000)
        self.assertEqual((len(page["items"]), page["has_more"]), (100, True))
        last = stats_cursor(self.state, WIDE, page["generation"], 10_000)
        self.assertEqual(self.stats("--cursor", last)["items"], [])

    def test_10001_sources_are_unavailable_or_expire_a_walk(self):
        self.many(10_001)
        page = self.stats()
        self.assertEqual((page["state"], page["items"]), ("unavailable", []))
        self.assertEqual(page["diagnostics"], [{"code": "history_limit", "message": "Statistics source history limit exceeded"}])
        error = self.stats_error("--cursor", stats_cursor(self.state, WIDE, "g", 0), code="snapshot_expired")
        self.assertEqual(error["message"], STATS_COVERAGE)

    def test_sources_outside_the_window_and_running_ones_count_toward_the_limit(self):
        self.world(*[seeds.task("s%05d" % n, status="running", exit_code=None, finished_at=None) for n in range(10_001)])
        self.assertEqual(self.stats()["diagnostics"][0]["code"], "history_limit")

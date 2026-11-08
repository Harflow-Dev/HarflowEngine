"""Black-box tests of the `statistics-page` cursor: decoding, binding, generation and offset.

The verdict table is `_decode_cursor` of statistics.py in HarflowMCP commit a426c5a, run with
python3 3.13.5 on the texts of feeds_cursors.stats_cases(). A text it accepts, bound to the request's
own root and window, must reach the generation check (the cursors here carry the generation "g",
which no store has, so the answer is snapshot_expired); every other text is "Invalid statistics
cursor". Binding compares integer microseconds and so is not Python's; those rows follow the plan.
"""

import os

import seed_support as seeds
from feeds_cursors import compact, encode, stats_cases, window_cases
from feeds_support import (
    STATS_COVERAGE, STATS_DECODE, STATS_EXPIRED, STATS_FOREIGN, STATS_ORDER, WIDE, FeedsCase, micros, settled,
    stats_cursor,
)

VERDICTS = {
    'plain': True,
    'unpadded': False,
    'empty text': False,
    'junk character': False,
    'trailing data': False,
    'not base64 text': False,
    'standard alphabet': True,
    'url alphabet': True,
    '4096 bytes': True,
    '4100 bytes': False,
    '4097 bytes of text': False,
    'non-ascii text': False,
    'v true': True,
    'v 1.0': True,
    'v 2': False,
    'v string': False,
    'v null': False,
    'v zero': False,
    'offset true': False,
    'offset false': False,
    'offset minus one': False,
    'offset minus zero': True,
    'offset 1.5': False,
    'offset 1e3': False,
    'offset 2.0': False,
    'offset string': False,
    'offset null': False,
    'offset zero': True,
    'offset three': True,
    'offset 2**64': True,
    'offset 2**64 minus one': True,
    'offset 10**30': True,
    'offset minus 2**64': False,
    'offset NaN': False,
    'offset Infinity': False,
    'offset list': False,
    'generation NaN': True,
    'generation number': True,
    'generation null': True,
    'generation list': True,
    'generation true': True,
    'generation empty': True,
    'list of the six keys': False,
    'extra key': False,
    'missing key': False,
    'missing offset': False,
    'string document': False,
    'null document': False,
    'empty object': False,
    'blank around the document': True,
    'duplicate offset last valid': True,
    'duplicate offset last invalid': False,
    'utf-8 byte order mark': True,
    'utf-16 with mark': True,
    'utf-16 little endian bare': True,
    'utf-32 big endian bare': True,
    'lone surrogate in generation': True,
    'raw control character in generation': False,
}


class CursorTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("c1", 0), settled("c2", 10), settled("c3", 20))
        self.window = (micros(WIDE[0]), micros(WIDE[1]))

    def test_the_table_covers_every_case(self):
        labels = [label for label, _ in stats_cases(self.state, *self.window)]
        self.assertEqual(len(labels), len(set(labels)))
        self.assertEqual(set(labels), set(VERDICTS))

    def test_each_cursor_text_is_decoded_as_the_reference_engine_decodes_it(self):
        for label, text in stats_cases(self.state, *self.window):
            with self.subTest(case=label):
                if VERDICTS[label]:
                    error = self.stats_error("--cursor", text, code="snapshot_expired")
                    self.assertEqual(error["message"], STATS_EXPIRED)
                else:
                    error = self.stats_error("--cursor", text, code="invalid_cursor")
                    self.assertEqual(error["message"], STATS_DECODE)

    def test_a_well_formed_cursor_for_another_root_or_window_is_refused_as_foreign(self):
        for label, text in window_cases(self.state, *self.window):
            with self.subTest(case=label):
                if label == "same window":
                    self.stats_error("--cursor", text, code="snapshot_expired")
                elif label == "since float of the same integer":
                    self.stats_error("--cursor", text, code="snapshot_expired")
                elif label == "v two":
                    self.assertEqual(self.stats_error("--cursor", text, code="invalid_cursor")["message"], STATS_DECODE)
                else:
                    error = self.stats_error("--cursor", text, code="invalid_cursor")
                    self.assertEqual(error["message"], STATS_FOREIGN)

    def test_a_cursor_for_another_window_is_foreign_even_when_the_bounds_differ_by_a_microsecond(self):
        other = ("2026-01-01T00:00:00.000001Z", WIDE[1])
        first = self.stats("--limit", "1")
        error = self.stats_error("--cursor", first["next_cursor"], code="invalid_cursor", window=other)
        self.assertEqual(error["message"], STATS_FOREIGN)

    def test_a_walk_survives_a_limit_change_and_resumes_at_the_offset(self):
        first = self.stats("--limit", "1")
        second = self.stats("--limit", "2", "--cursor", first["next_cursor"])
        self.assertEqual([i["task_id"] for i in first["items"] + second["items"]], ["c1", "c2", "c3"])
        self.assertEqual((second["has_more"], second["next_cursor"]), (False, None))
        self.assertEqual(second["generation"], first["generation"])

    def test_the_offset_may_reach_the_end_but_not_pass_it(self):
        generation = self.stats()["generation"]
        end = self.stats("--cursor", stats_cursor(self.state, WIDE, generation, 3))
        self.assertEqual((end["state"], end["items"], end["has_more"]), ("ready", [], False))
        for offset in (4, 2 ** 63, 2 ** 64, 2 ** 64 + 1, 10 ** 30):
            with self.subTest(offset=offset):
                error = self.stats_error("--cursor", stats_cursor(self.state, WIDE, generation, offset), code="invalid_cursor")
                self.assertEqual(error["message"], STATS_ORDER)
                stale = self.stats_error("--cursor", stats_cursor(self.state, WIDE, "stale", offset), code="snapshot_expired")
                self.assertEqual(stale["message"], STATS_EXPIRED)

    def test_the_generation_is_judged_before_the_offset(self):
        error = self.stats_error("--cursor", stats_cursor(self.state, WIDE, "0" * 64, 99), code="snapshot_expired")
        self.assertEqual(error["message"], STATS_EXPIRED)

    def test_the_big_offset_spellings_of_the_plan(self):
        generation = self.stats()["generation"]
        for offset in ("18446744073709551616", "-0"):
            text = ('{"v":1,"root":%s,"since":%d,"before":%d,"generation":"%s","offset":%s}'
                    % (__import__("json").dumps(self.state), micros(WIDE[0]), micros(WIDE[1]), generation, offset))
            done = self.stats("--cursor", encode(text.encode())) if offset == "-0" else None
            if offset == "-0":
                self.assertEqual(len(done["items"]), 3)
            else:
                error = self.stats_error("--cursor", encode(text.encode()), code="invalid_cursor")
                self.assertEqual(error["message"], STATS_ORDER)
                stale = text.replace(generation, "x")
                self.stats_error("--cursor", encode(stale.encode()), code="snapshot_expired")

    def test_a_nan_or_non_text_generation_expires_the_walk(self):
        for generation in ("NaN", "7", "null", '["g"]'):
            text = ('{"v":1,"root":"%s","since":%d,"before":%d,"generation":%s,"offset":0}'
                    % (self.state, micros(WIDE[0]), micros(WIDE[1]), generation))
            self.stats_error("--cursor", encode(text.encode()), code="snapshot_expired")

    def test_deeply_nested_json_in_an_untyped_member_is_read(self):
        # json.loads of python3 3.13.5 reads 5,000 levels; a 4,096-character cursor carries 3,072 bytes of JSON, about 1,450 levels.
        for depth in (100, 1000, 1400):
            nested = "[" * depth + "]" * depth
            text = ('{"v":1,"root":"%s","since":%d,"before":%d,"generation":%s,"offset":0}'
                    % (self.state, micros(WIDE[0]), micros(WIDE[1]), nested))
            self.assertLessEqual(len(encode(text.encode())), 4096)
            with self.subTest(depth=depth):
                self.stats_error("--cursor", encode(text.encode()), code="snapshot_expired")

    def test_the_cursor_text_the_engine_issues_is_the_documented_shape(self):
        first = self.stats("--limit", "2")
        from feeds_support import decode_cursor
        value = decode_cursor(first["next_cursor"])
        self.assertEqual(value, {"v": 1, "root": self.state, "since": self.window[0], "before": self.window[1],
                                 "generation": first["generation"], "offset": 2})
        self.assertEqual(compact(value).decode(), __import__("base64").urlsafe_b64decode(first["next_cursor"]).decode())


class CursorStoreStateTests(FeedsCase):
    def test_a_missing_or_empty_store_checks_the_empty_generation_then_the_offset(self):
        from feeds_support import EMPTY_GENERATION
        absent = os.path.join(self.base, "nothing")
        window = WIDE
        good = stats_cursor(absent, window, EMPTY_GENERATION, 0)
        page = self.stats("--cursor", good, root=absent)
        self.assertEqual((page["state"], page["items"], page["generation"]), ("ready", [], EMPTY_GENERATION))
        self.assertEqual(self.stats_error("--cursor", stats_cursor(absent, window, "x", 0), code="snapshot_expired", root=absent)["message"], STATS_EXPIRED)
        self.assertEqual(self.stats_error("--cursor", stats_cursor(absent, window, EMPTY_GENERATION, 1), code="invalid_cursor", root=absent)["message"], STATS_ORDER)
        self.stats_error("--cursor", "bogus", code="invalid_cursor", root=absent)
        self.stats_error("--cursor", stats_cursor(absent + "x", window, EMPTY_GENERATION, 0), code="invalid_cursor", root=absent)
        self.assertFalse(os.path.lexists(absent))

    def test_a_store_without_a_task_in_the_window_has_the_empty_generation_too(self):
        from feeds_support import EMPTY_GENERATION
        self.world(settled("c1"))
        other = ("2030-01-01T00:00:00Z", "2031-01-01T00:00:00Z")
        page = self.stats(window=other)
        self.assertEqual(page["generation"], EMPTY_GENERATION)
        self.stats("--cursor", stats_cursor(self.state, other, EMPTY_GENERATION, 0), window=other)
        self.stats_error("--cursor", stats_cursor(self.state, other, EMPTY_GENERATION, 1), code="invalid_cursor", window=other)

    def test_a_scan_that_cannot_finish_expires_a_walk_and_leaves_a_fresh_read_unavailable(self):
        count = 10_001
        tasks = [seeds.task("s%05d" % n, started_at="2026-01-10T10:00:00+00:00", finished_at="2026-01-10T10:01:00+00:00")
                 for n in range(count)]
        self.world(*tasks)
        page = self.stats()
        self.assertEqual(page["state"], "unavailable")
        self.assertEqual(page["diagnostics"], [{"code": "history_limit", "message": "Statistics source history limit exceeded"}])
        error = self.stats_error("--cursor", stats_cursor(self.state, WIDE, "g", 0), code="snapshot_expired")
        self.assertEqual(error["message"], STATS_COVERAGE)


class BudgetTests(FeedsCase):
    """`HARFLOW_STATISTICS_BUDGET_MS` shortens the two-second statistics budget so that it can run out."""

    ENV = {"HARFLOW_STATISTICS_BUDGET_MS": "0"}

    def setUp(self):
        super().setUp()
        self.world(settled("b1", 0), settled("b2", 10))

    def test_an_expired_budget_leaves_a_fresh_read_unavailable_and_exit_zero(self):
        page = self.stats(env=self.ENV)
        self.assertEqual((page["state"], page["items"], page["generation"]), ("unavailable", [], None))
        self.assertEqual(page["diagnostics"], [{"code": "discovery_budget", "message": "No source classification fits the foreground deadline"}])

    def test_an_expired_budget_expires_a_cursor_walk_and_is_never_a_busy_store(self):
        first = self.stats("--limit", "1")
        error = self.stats_error("--cursor", first["next_cursor"], code="snapshot_expired", env=self.ENV)
        self.assertEqual(error["message"], STATS_COVERAGE)

    def test_a_budget_that_is_not_a_number_or_is_ample_changes_nothing(self):
        for value in ("", "x", "-1", "60000"):
            with self.subTest(value=value):
                self.assertEqual(self.stats(env={"HARFLOW_STATISTICS_BUDGET_MS": value})["state"], "ready")

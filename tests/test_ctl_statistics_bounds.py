"""Black-box tests of the `statistics-page` bounds: which spellings are instants, and which instant.

Each row is (text, parses, aware, microseconds). `parses` is whether python3 3.13.5
`datetime.fromisoformat(text.replace("Z", "+00:00"))` accepts the text, `aware` whether the result
carries an offset (statistics.py of HarflowMCP commit a426c5a refuses naive values), and the
microseconds are the exact stdlib difference to the epoch, `(d - epoch) // timedelta(microseconds=1)`.
The parser verdict and the bound verdict are separate columns: a bound is valid only when both hold.
The microseconds are observed in the cursor the engine issues, which binds the window as integers.
"""

import base64
import json

import seed_support as seeds
from feeds_support import STATS_PARAMS, FeedsCase, micros, stamp

EARLY = "0001-01-01T00:00:00+14:00"
LATE = "9999-12-31T23:59:59.999999-14:00"
FIRST_START = "2026-01-20T10:00:00+00:00"
ROWS = (
    ('2026-01-10T10:00:00Z', True, True, 1768039200000000),
    ('2026-01-10T10:00:00z', False, False, None),
    ('2026-01-10T10:00:00+07:00', True, True, 1768014000000000),
    ('2026-01-10T10:00:00+0700', True, True, 1768014000000000),
    ('2026-01-10T10:00:00+07', True, True, 1768014000000000),
    ('2026-01-10T10:00:00+07:00:00', True, True, 1768014000000000),
    ('2026-01-10T10:00:00+07:00:30', True, True, 1768013970000000),
    ('2026-01-10T10:00:00+07:00:30.5', True, True, 1768013969500000),
    ('2026-01-10 10:00:00Z', True, True, 1768039200000000),
    ('2026-01-10_10:00:00Z', True, True, 1768039200000000),
    ('2026-01-10x10:00:00Z', True, True, 1768039200000000),
    ('2026-01-10é10:00:00Z', True, True, 1768039200000000),
    ('2026-01-10中10:00:00Z', True, True, 1768039200000000),
    ('20260110T100000Z', True, True, 1768039200000000),
    ('20260110T100000+0700', True, True, 1768014000000000),
    ('20260110T100000+07', True, True, 1768014000000000),
    ('20260110T100000.5Z', True, True, 1768039200500000),
    ('20260110T100000.5+0700', True, True, 1768014000500000),
    ('2026-01-10T100000Z', True, True, 1768039200000000),
    ('20260110T10:00:00Z', True, True, 1768039200000000),
    ('2026-W02-6T10:00:00Z', True, True, 1768039200000000),
    ('2026-W02-6T10:00:00+00:00', True, True, 1768039200000000),
    ('2026W026T100000Z', True, True, 1768039200000000),
    ('2026-W02T10:00:00Z', True, True, 1767607200000000),
    ('2026-W02', True, False, None),
    ('2026-W02-6', True, False, None),
    ('2026-W02-6T10:00:00', True, False, None),
    ('2026-010T10:00:00Z', False, False, None),
    ('2026010T100000Z', False, False, None),
    ('2026-01-10T10:00:00.1234567Z', True, True, 1768039200123456),
    ('2026-01-10T10:00:00.123456789+00:00', True, True, 1768039200123456),
    ('2026-01-10T10:00:00.5Z', True, True, 1768039200500000),
    ('2026-01-10T10:00:00,5Z', True, True, 1768039200500000),
    ('2026-01-10T10:00:00.Z', True, True, 1768039200000000),
    ('2026-01-10T10:00:00.000001Z', True, True, 1768039200000001),
    ('2026-01-10', True, False, None),
    ('2026-01-10Z', True, False, None),
    ('2026-01-10T10:00:00', True, False, None),
    ('2026-01-10T10', True, False, None),
    ('2026-01-10T10Z', True, True, 1768039200000000),
    ('2026-01-10T10+07:00', True, True, 1768014000000000),
    ('2026-01-10T10:00Z', True, True, 1768039200000000),
    ('2026-01-10T10:00+0700', True, True, 1768014000000000),
    ('2026-01-10T10:00:00 +07:00', True, True, 1768014000000000),
    ('2026-01-10T10:00:00+07:00 ', False, False, None),
    ('2026-01-10T10:00:00Z ', False, False, None),
    (' 2026-01-10T10:00:00Z', False, False, None),
    ('2026-01-10T10:00:00+07:00Z', False, False, None),
    ('2026-01-10T10:00:00Z+07:00', False, False, None),
    ('2026-01-10T10:00:00.5+07:00:00.25', True, True, 1768014000250000),
    ('2026-01-10T10:00.5+07:00', True, True, 1768014000500000),
    ('2026-01-10T10:00:00+00', True, True, 1768039200000000),
    ('2026-01-10T10:00:00+053000', True, True, 1768019400000000),
    ('2026-01-10T10:00:00+0530', True, True, 1768019400000000),
    ('2026-01-10T10:00:00-05:30', True, True, 1768059000000000),
    ('2026-01-10T10:00:00+24:00', False, False, None),
    ('2026-01-10T10:00:00-24:00', False, False, None),
    ('2026-01-10T10:00:00+23:59', True, True, 1767952860000000),
    ('2026-01-10T10:00:00+23:59:59', True, True, 1767952801000000),
    ('2026-01-10T10:00:00+99:00', False, False, None),
    ('2027-W53-1T00:00:00Z', False, False, None),
    ('2026-W53-1T00:00:00Z', True, True, 1798416000000000),
    ('2020-W53-1T00:00:00Z', True, True, 1609113600000000),
    ('2026-01T00:00:00Z', False, False, None),
    ('2026-01-10T17:0Z', False, False, None),
    ('2026-01-10T24:00:00Z', False, False, None),
    ('2026-01-10T23:59:60Z', False, False, None),
    ('2026-01-10T10:60:00Z', False, False, None),
    ('2026-02-29T10:00:00Z', False, False, None),
    ('2028-02-29T10:00:00Z', True, True, 1835431200000000),
    ('2026-13-01T00:00:00Z', False, False, None),
    ('2026-00-10T00:00:00Z', False, False, None),
    ('2026-01-00T00:00:00Z', False, False, None),
    ('2026-01-32T00:00:00Z', False, False, None),
    ('0000-01-01T00:00:00Z', False, False, None),
    ('0001-01-01T00:00:00Z', True, True, -62135596800000000),
    ('0001-01-01T00:00:00+01:00', True, True, -62135600400000000),
    ('0001-01-01T00:00:00-01:00', True, True, -62135593200000000),
    ('9999-12-31T23:59:59.999999Z', True, True, 253402300799999999),
    ('9999-12-31T23:59-01:00', True, True, 253402304340000000),
    ('9999-12-31T23:59:59+00:00', True, True, 253402300799000000),
    ('10000-01-01T00:00:00Z', False, False, None),
    ('1969-12-31T23:59:59.999999Z', True, True, -1),
    ('1969-12-31T23:59:59.5Z', True, True, -500000),
    ('1969-12-31T23:59:58.5Z', True, True, -1500000),
    ('1970-01-01T00:00:00Z', True, True, 0),
    ('1970-01-01T00:00:00.000001Z', True, True, 1),
    ('', False, False, None),
    ('nonsense', False, False, None),
    ('T10:00:00Z', False, False, None),
    ('2026-01-10T', False, False, None),
    ('2026-1-10T10:00:00Z', False, False, None),
    ('26-01-10T10:00:00Z', False, False, None),
    ('+2026-01-10T10:00:00Z', False, False, None),
    ('2026-01-10T10:00:00+7:00', False, False, None),
    ('2026-01-10T10:00:00+07:0', False, False, None),
    ('2026-01-10T1:00:00Z', False, False, None),
    ('2026-01-10T10:00:00 Z', True, True, 1768039200000000),
    ('2026-01-10T10:00:00ZZ', False, False, None),
    ('2026-01-10T10:00:00\xa0Z', False, False, None),
    ('２０２６-01-10T10:00:00Z', False, False, None),
    ('2026-01-10T10:00:00+０７:00', False, False, None),
    # Probed again for the S5 review: ordinal dates, fractions of the hour or minute, stray characters.
    ('2021-045T00:00+00:00', False, False, None),
    ('2021034T0000+0000', False, False, None),
    ('2026-01-10T12.5+00:00', True, True, 1768046400500000),
    ('2026-01-10T12:30.5+00:00', True, True, 1768048200500000),
    ('2026-01-10T12:00x+00:00', True, True, 1768046400000000),
    ('2026-01-10T12:00:00.5x+00:00', False, False, None),
    ('2026-01-10T12:30.+00:00', True, True, 1768048200000000),
    ('2026-01-10T12:30.', False, False, None),
    ('2026-01-10T12:00:00 +00:00', True, True, 1768046400000000),
    ('2026-01-10T12:00:00  +00:00', False, False, None),
    ('2026-01-01T12:00:00.5x+00:00', False, False, None),
    ('2026-01-01T12:00x+00:00', True, True, 1767268800000000),
    ('2026-01-01T12:00:00.512345x+00:00', True, True, 1767268800512345),
    ('2026-01-01T12:00:00.5123456x+00:00', True, True, 1767268800512345),
)


def bound_ok(parses, aware):
    return parses and aware


class BoundsTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(seeds.task("w1", started_at=FIRST_START, finished_at="2026-01-20T10:05:00+00:00"),
                   seeds.task("w2", started_at="2026-01-21T10:00:00+00:00", finished_at="2026-01-21T10:05:00+00:00"))

    def windows(self, text, value):
        """The two placements of a bound: as the lower one, or as the upper one."""
        if value is not None and value >= micros(FIRST_START):
            return [("before", (EARLY, text))]
        if value is not None:
            return [("since", (text, LATE))]
        return [("since", (text, LATE)), ("before", (EARLY, text))]

    def test_the_table_has_each_kind_of_row(self):
        self.assertEqual(len({row[0] for row in ROWS}), len(ROWS))
        kinds = {(parses, aware) for _, parses, aware, _ in ROWS}
        self.assertEqual(kinds, {(True, True), (True, False), (False, False)})
        for text, parses, aware, value in ROWS:
            self.assertEqual(value is not None, bound_ok(parses, aware), text)

    def test_each_text_is_a_bound_exactly_when_the_reference_parser_gives_an_aware_instant(self):
        for text, parses, aware, value in ROWS:
            for place, window in self.windows(text, value):
                with self.subTest(text=text, place=place):
                    if bound_ok(parses, aware):
                        page = self.stats("--limit", "1", window=window)
                        self.assertEqual(page["state"], "ready", text)
                        self.assertEqual(len(page["items"]), 1)
                        cursor = self.cursor_of(page)
                        self.assertEqual(cursor[place], value)
                    else:
                        error = self.stats_error(code="invalid_params", window=window)
                        self.assertEqual(error["message"], STATS_PARAMS)

    def cursor_of(self, page):
        text = page["next_cursor"]
        return json.loads(base64.urlsafe_b64decode(text + "=" * (-len(text) % 4)))

    def test_the_cursor_binds_both_bounds_as_integer_microseconds(self):
        window = ("2026-01-20T10:00:00+00:00", "2026-01-22T00:00:00Z")
        cursor = self.cursor_of(self.stats("--limit", "1", window=window))
        self.assertEqual((cursor["since"], cursor["before"]), (micros(window[0]), micros(window[1])))
        self.assertEqual({key for key in cursor}, {"v", "root", "since", "before", "generation", "offset"})
        self.assertEqual((cursor["v"], cursor["root"], cursor["offset"]), (1, self.state, 1))

    def test_a_bound_that_is_not_text_for_the_parser_is_refused_whatever_the_other_is(self):
        self.stats_error(code="invalid_params", window=("2026-01-01T00:00:00", "2027-01-01T00:00:00"))
        self.stats_error(code="invalid_params", window=("2026-01-01T00:00:00Z", "2027-01-01T00:00:00"))
        self.stats_error(code="invalid_params", window=("2026-01-01T00:00:00", "2027-01-01T00:00:00Z"))


class OrderAndLimitTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(seeds.task("w1", started_at=FIRST_START, finished_at="2026-01-20T10:05:00+00:00"))

    def test_the_window_must_be_strictly_ordered(self):
        same = "2026-01-21T00:00:00Z"
        for window in ((same, same), ("2026-01-22T00:00:00Z", "2026-01-21T00:00:00Z"),
                       ("2026-01-21T00:00:00Z", "2026-01-20T23:00:00+02:00"), ("nonsense", "nonsense")):
            with self.subTest(window=window):
                error = self.stats_error(code="invalid_params", window=window)
                self.assertEqual(error["message"], STATS_PARAMS)
        self.assertEqual(self.stats(window=("2026-01-21T00:00:00Z", "2026-01-21T00:00:00.000001Z"))["state"], "ready")
        # The same instant written with two offsets is not ordered.
        self.stats_error(code="invalid_params", window=("2026-01-21T00:00:00+00:00", "2026-01-21T07:00:00+07:00"))

    def test_limits_outside_1_to_100_are_invalid_params(self):
        for limit in ("0", "101", "-1", "1000", "9223372036854775807"):
            with self.subTest(limit=limit):
                error = self.stats_error("--limit", limit, code="invalid_params")
                self.assertEqual(error["message"], STATS_PARAMS)

    def test_a_limit_beyond_64_bits_is_an_integer_to_argparse_and_so_out_of_range(self):
        # argparse reads any digits as an int, so the reference engine answers invalid_params
        # (exit 1).
        for limit in ("9223372036854775808", "99999999999999999999", "-9223372036854775809"):
            with self.subTest(limit=limit):
                self.stats_error("--limit", limit, code="invalid_params")

    def test_missing_bounds_and_text_limits_are_usage_errors(self):
        for args in (("--before", "2027-01-01T00:00:00Z"), ("--since", "2026-01-01T00:00:00Z"), ()):
            done = self.read("statistics-page", *args, "--json")
            self.assertEqual(done.returncode, 2, args)
            self.assertEqual(json.loads(done.stdout)["error"]["code"], "usage")
            self.assertIn("the following arguments are required", done.stderr)
        done = self.read("statistics-page", "--since", "x", "--before", "y", "--limit", "z", "--json")
        self.assertEqual(done.returncode, 2)

"""Black-box tests of `--limit` and `HARFLOW_RETENTION_DAYS` as Python's `int(str)` reads them.

argparse's `type=int` and the retention rule both call `int()`; python3 3.13.5's own `int()` and
`unicodedata` are the oracle here, so the expected verdict of every text is computed by the stdlib
call the reference engine makes, never by the engine.
"""

import json
import unicodedata

import seed_support as seeds
from feeds_support import WIDE, FeedsCase, settled

TEXTS = (
    "5", " 5 ", "+5", "-5", "0", "-0", "00012", "1_0", "1__0", "_1", "1_", "5.0", "0x10", "1e2", "", " ",
    "\x1c5", "5\x1f", "\x1c5\x1f", "\x857", " 7", "\x0b7\x0c", "\x0b5\x0c", " 5 ", "٥", "١٠", "١_٠", "５",
    "①", "²", "٥x", "+ 5", "- 5", "++5", "5 5", "​5",
    "9" * 19, "9" * 20, "9" * 4300, "9" * 4301, "0" * 4299 + "7", "0" * 4300 + "7", "0" * 5000 + "7", "-" + "9" * 4300,
)


def python_int(text):
    try:
        return int(text)
    except ValueError:
        return None


def nd_runs():
    """(first code point, ten characters) of every run of decimal digits in the interpreter's Unicode."""
    found = {}
    for code in range(0x110000):
        character = chr(code)
        if unicodedata.category(character) == "Nd":
            found.setdefault(code - unicodedata.digit(character), [None] * 10)[unicodedata.digit(character)] = character
    return found


class LeadingZeroTests(FeedsCase):
    def test_python_counts_every_digit_leading_zeros_included(self):
        self.assertEqual(python_int("0" * 4299 + "7"), 7)
        self.assertIsNone(python_int("0" * 4300 + "7"))


class LimitTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("l1"))

    def feed_limit(self, text):
        return self.read("interaction-events", "--limit", text, "--json")

    def test_each_text_is_usage_in_range_or_out_of_range_as_int_decides(self):
        for text in TEXTS:
            value = python_int(text)
            with self.subTest(text=text[:12], size=len(text)):
                done = self.feed_limit(text)
                if value is None:
                    self.assertEqual(done.returncode, 2, done.stdout[:200])
                    self.assertEqual(json.loads(done.stdout)["error"]["code"], "usage")
                elif 1 <= value <= 100:
                    self.assertEqual(done.returncode, 0, done.stdout[:200])
                else:
                    self.assertEqual(done.returncode, 1)
                    self.assertEqual(json.loads(done.stdout)["error"]["code"], "interaction_events_error")

    def test_every_script_of_decimal_digits_is_read(self):
        for zero, digits in nd_runs().items():
            self.assertNotIn(None, digits, hex(zero))
            for text, value in ((digits[1] + digits[0], 10), (digits[9] + digits[9], 99), (digits[1], 1)):
                with self.subTest(zero=hex(zero), text=text):
                    self.assertEqual(int(text), value)
                    done = self.feed_limit(text)
                    self.assertEqual(done.returncode, 0, done.stdout[:200])

    def test_statistics_and_recovery_follow_the_same_reading(self):
        for text in ("99999999999999999999", "-99999999999999999999", "9" * 4300, "١٠٠١"):
            with self.subTest(text=text[:12]):
                done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--limit", text, "--json")
                self.assertEqual(done.returncode, 1, done.stdout[:200])
                self.assertEqual(json.loads(done.stdout)["error"]["code"], "invalid_params")
                done = self.read("recovery-activity", seeds.A, "--limit", text, "--json")
                self.assertEqual(done.returncode, 1)  # the task is unknown; the limit is not judged
                self.assertEqual(json.loads(done.stdout)["error"]["code"], "unknown_task")
        for text in ("1_0", "١٠"):
            done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--limit", text, "--json")
            self.assertEqual(done.returncode, 0, text)
        for text in ("1__0", "9" * 4301, "x"):
            for command in (("statistics-page", "--since", WIDE[0], "--before", WIDE[1]), ("recovery-activity", seeds.A)):
                self.assertEqual(self.read(*command, "--limit", text, "--json").returncode, 2, (command[0], text[:5]))


class RetentionIntTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("r1"))

    def days(self, text):
        done = self.read("statistics-page", "--since", WIDE[0], "--before", WIDE[1], "--json",
                         env={"HARFLOW_RETENTION_DAYS": text})
        self.assertEqual(done.returncode, 0, done.stdout[:200])
        return done.stdout, json.loads(done.stdout)["result"]["retention"]

    def test_each_text_gives_the_policy_python_derives_from_int(self):
        for text in TEXTS:
            if "\x00" in text:
                continue
            value = python_int(text)
            if value is None or value < 0:
                expected = {"state": "unknown", "days": None}
            elif value == 0:
                expected = {"state": "disabled", "days": None}
            else:
                expected = {"state": "days", "days": value}
            with self.subTest(text=text[:12], size=len(text)):
                raw, got = self.days(text)
                self.assertEqual(got, expected)
                if expected["days"] is not None:
                    self.assertIn('"days": %d,' % expected["days"], raw)

    def test_every_script_of_decimal_digits_is_read(self):
        for zero, digits in nd_runs().items():
            text = digits[1] + digits[2]
            with self.subTest(zero=hex(zero)):
                self.assertEqual(self.days(text)[1], {"state": "days", "days": 12})

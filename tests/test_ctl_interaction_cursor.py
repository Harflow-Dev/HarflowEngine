"""Black-box tests of the `interaction-events` cursor and its error order.

The verdict table is the answer of `_parse` in interaction_events.py of HarflowMCP commit a426c5a,
run with python3 3.13.5 on exactly the texts of feeds_cursors.feed_cases(). A text the reference
engine accepts must be read (exit 0); every other text is an `interaction_events_error`.
"""

import os

from feeds_cursors import feed_cases
from feeds_support import IE_DIAG, IE_LIMIT, FeedsCase
import seed_support as seeds
from seed_support import A, B

VERDICTS = {
    'padded': True,
    'unpadded': False,
    'empty text': False,
    'space inside': False,
    'newline inside': False,
    'junk character': False,
    'trailing data': False,
    'two documents': False,
    'not base64 text': False,
    'non-ascii text': False,
    'standard alphabet with plus': True,
    'standard alphabet with slash': True,
    'url alphabet with minus or underscore': True,
    'padding missing by one': False,
    'padding doubled': False,
    '4096 characters': True,
    '4100 characters': False,
    '4097 characters': False,
    'v true': True,
    'v 1.0': True,
    'v 1e0': True,
    'v 1.0000000000000001': True,
    'v 2': False,
    'v string': False,
    'v null': False,
    'v list': False,
    'v zero': False,
    'v minus zero': False,
    'v huge': False,
    'v 1e400': False,
    'v NaN': False,
    'v Infinity': False,
    'v false': False,
    'phase workflow': True,
    'phase x': False,
    'phase upper': False,
    'phase empty': False,
    'phase list': False,
    'after dotted': True,
    'after 100': True,
    'after 101': False,
    'after underscore first': False,
    'after dash first': False,
    'after dot first': False,
    'after space': False,
    'after accented': False,
    'after number': False,
    'after newline end': False,
    'after digit first': True,
    'after 512': False,
    'after 513': False,
    'source 100': True,
    'source 101': False,
    'source slash': False,
    'source number': False,
    'event 512': True,
    'event 513': False,
    'event accented 512': True,
    'event emoji 512': True,
    'event emoji 513': False,
    'event number': False,
    'event null': False,
    'event quote and backslash': True,
    'list of the five keys': False,
    'extra key': False,
    'missing key': False,
    'string document': False,
    'null document': False,
    'number document': False,
    'empty object': False,
    'nan event': False,
    'blank around the document': True,
    'raw control character in a string': False,
    'lone surrogate escape in event': True,
    'lone surrogate escape in after': False,
    'duplicate key last valid': True,
    'duplicate key last invalid': False,
    'duplicate v last wrong': False,
    'utf-8 byte order mark': True,
    'utf-16 with mark': True,
    'utf-16 little endian bare': True,
    'utf-16 big endian bare': True,
    'utf-32 with mark': True,
    'utf-32 little endian bare': True,
    'utf-32 big endian bare': True,
    'utf-16 odd length': False,
    'invalid utf-8': False,
    'overlong utf-8': False,
    'surrogate in utf-8': True,
    'second mark after the first': False,
    'deep nesting in event': False,
}


class FeedCursorTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init()
        self.add(A)

    def test_the_table_covers_every_case(self):
        labels = [label for label, _ in feed_cases()]
        self.assertEqual(len(labels), len(set(labels)))
        self.assertEqual(set(labels), set(VERDICTS))

    def test_each_cursor_text_is_accepted_or_refused_as_the_reference_engine_does(self):
        for label, text in feed_cases():
            with self.subTest(case=label):
                if VERDICTS[label]:
                    page = self.feed("--cursor", text)
                    self.assertIn("cycle_complete", page)
                else:
                    error = self.feed_error("--cursor", text, code="interaction_events_error")
                    self.assertEqual(error["message"], IE_DIAG)

    def test_the_cursor_is_not_bound_to_a_root(self):
        self.add(B)
        first = self.feed("--limit", "1")
        self.assertIsNotNone(first["next_cursor"])
        elsewhere = os.path.join(self.base, "elsewhere")
        self.init(seeds.task(A), seeds.task(B), root=elsewhere)
        page = self.feed("--cursor", first["next_cursor"], root=elsewhere)
        self.assertEqual([row["task_id"] for row in page["events"]], [B])


class FeedErrorTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init()
        self.add(A)

    def test_limits_outside_1_to_100_are_errors_after_the_cursor_is_judged(self):
        for limit in ("0", "101", "-1", "1000"):
            with self.subTest(limit=limit):
                error = self.feed_error("--limit", limit, code="interaction_events_error")
                self.assertEqual(error["message"], IE_LIMIT)
        self.assertEqual(len(self.feed("--limit", "100")["events"]), 1)

    def test_the_cursor_is_judged_before_the_limit(self):
        error = self.feed_error("--cursor", "zzz", "--limit", "0", code="interaction_events_error")
        self.assertEqual(error["message"], IE_DIAG)
        error = self.feed_error("--cursor", "zzz", "--limit", "500", code="interaction_events_error")
        self.assertEqual(error["message"], IE_DIAG)

    def test_a_non_integer_limit_is_a_usage_error(self):
        for value in ("x", "1.5", ""):
            done = self.read("interaction-events", "--limit", value, "--json")
            self.assertEqual(done.returncode, 2, value)

    def test_a_missing_root_validates_the_cursor_then_the_limit_and_answers_empty(self):
        absent = os.path.join(self.base, "nothing")
        empty = {"events": [], "next_cursor": None, "cycle_complete": True, "history_incomplete": False, "gaps": []}
        self.assertEqual(self.feed(root=absent), empty)
        self.feed_error("--cursor", "zzz", code="interaction_events_error", root=absent)
        self.feed_error("--limit", "0", code="interaction_events_error", root=absent)
        error = self.feed_error("--cursor", "zzz", "--limit", "0", code="interaction_events_error", root=absent)
        self.assertEqual(error["message"], IE_DIAG)
        self.assertEqual(self.feed("--limit", "100", root=absent), empty)
        self.assertFalse(os.path.lexists(absent))

    def test_an_empty_directory_root_answers_empty_and_stays_empty(self):
        empty_root = os.path.join(self.base, "empty")
        os.mkdir(empty_root)
        self.assertEqual(self.feed(root=empty_root)["events"], [])
        self.assertEqual(os.listdir(empty_root), [])

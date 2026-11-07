"""Black-box tests of how a `task-events` page is fitted to the document bound.

The bound is 262,144 bytes less a 512 byte reserve, sized as the reference engine writes the
document: `json.dumps` with default separators and `ensure_ascii`, plus the newline. The tests
build logs whose page is exactly at the bound and one byte over it, in every entry mode, then
walk whole logs and check that every event is served once.
"""

import json
import os

import seed_support as seeds
from history_support import DOCUMENT_BYTES, RESERVE_BYTES, HistoryCase, event, printed
from seed_support import A

CAP = DOCUMENT_BYTES - RESERVE_BYTES
assert CAP == 261_632
LINES = 125
FIRST_FILLER = 100
TUNED = LINES // 2
MODES = ("newest", "oldest", "older", "newer")
# The sequence numbers each entry mode serves when `LINES` lines fit, and when one line less does.
FULL = {
    "newest": range(2, LINES + 2), "oldest": range(0, LINES),
    "older": range(1, LINES + 1), "newer": range(1, LINES + 1),
}
SHORT = {
    "newest": range(3, LINES + 2), "oldest": range(0, LINES - 1),
    "older": range(2, LINES + 1), "newer": range(1, LINES),
}
# How many of the large events each mode's page holds.
LARGE = {"newest": LINES - 1, "oldest": LINES - 1, "older": LINES, "newer": LINES}


def size_of(done):
    return printed(done)[1]


class FitCase(HistoryCase):
    def make(self, name, events, root=None):
        root = root or os.path.join(self.base, name)
        self.world(seeds.task(A), events={A: events}, root=root)
        return root

    def read_page(self, root, *args):
        done = self.read("task-events", A, "--json", *args, root=root)
        self.assertEqual((done.returncode, done.stderr), (0, ""), done.stdout)
        return done

    def all_sizes_fit(self, pages):
        for done in pages:
            self.assertLessEqual(size_of(done), CAP)


class BoundaryTests(FitCase):
    """A page of `LINES` lines whose document is exactly CAP bytes is served whole, and one byte more is not."""

    def log(self, filler, tuned):
        events = [event(0, "notice", "head")]
        for seq in range(1, LINES + 1):
            text = "a" * tuned if seq == TUNED else "\u00e9" * filler
            events.append(event(seq, "assistant_text", text))
        events.append(event(LINES + 1, "notice", "tail"))
        return events

    def read_mode(self, mode, root):
        count = str(LINES)
        if mode == "newest":
            return self.read_page(root, "--limit", count)
        if mode == "oldest":
            return self.read_page(root, "--from", "oldest", "--limit", count)
        if mode == "older":
            first = json.loads(self.read_page(root, "--limit", "1").stdout)["result"]
            return self.read_page(root, "--cursor", first["older_cursor"], "--limit", count)
        first = json.loads(self.read_page(root, "--from", "oldest", "--limit", "1").stdout)["result"]
        return self.read_page(root, "--cursor", first["newer_cursor"], "--limit", count)

    def whole(self, mode, filler, name):
        """The size of the page of this mode when it is whole, else None."""
        root = self.make(name, self.log(filler, 0))
        done = self.read_mode(mode, root)
        served = len(json.loads(done.stdout)["result"]["events"])
        return size_of(done) if served == LINES else None

    def tuned_logs(self, mode):
        """Two roots whose full page is CAP and CAP + 1 bytes: the largest whole filler by bisection,
        then the one tuned event takes up the rest. The filler is two bytes stored and six bytes
        written per character, so the document bound is reached before the stored-byte cap."""
        low, high = FIRST_FILLER, 2001
        self.assertIsNotNone(self.whole(mode, low, f"{mode}-low"))
        while high - low > 1:
            middle = (low + high) // 2
            found = self.whole(mode, middle, f"{mode}-{middle}")
            low, high = (middle, high) if found is not None else (low, middle)
        base = self.whole(mode, low, f"{mode}-base")
        tuned = CAP - base
        self.assertTrue(0 <= tuned <= 2000, tuned)
        self.assertLess(tuned, 1000)
        return (self.make(f"{mode}-fit", self.log(low, tuned)),
                self.make(f"{mode}-over", self.log(low, tuned + 1)))

    def test_a_page_exactly_at_the_bound_is_whole_in_every_entry_mode(self):
        for mode in MODES:
            with self.subTest(mode=mode):
                fit, _ = self.tuned_logs(mode)
                done = self.read_mode(mode, fit)
                self.assertEqual(size_of(done), CAP)
                page = json.loads(done.stdout)["result"]
                self.assertEqual(self.seqs(page), list(FULL[mode]))

    def test_one_byte_over_the_bound_drops_one_line_from_the_far_side(self):
        for mode in MODES:
            with self.subTest(mode=mode):
                _, over = self.tuned_logs(mode)
                done = self.read_mode(mode, over)
                self.assertLessEqual(size_of(done), CAP)
                page = json.loads(done.stdout)["result"]
                self.assertEqual(self.seqs(page), list(SHORT[mode]))

    def test_the_dropped_line_is_the_next_one_when_the_cursor_is_followed(self):
        for mode in MODES:
            with self.subTest(mode=mode):
                _, over = self.tuned_logs(mode)
                page = json.loads(self.read_mode(mode, over).stdout)["result"]
                if mode in ("newest", "older"):
                    follow, expected = page["older_cursor"], SHORT[mode][0] - 1
                    nxt = self.page(A, "--cursor", follow, "--limit", "1", root=over)
                    self.assertEqual(self.seqs(nxt), [expected])
                    self.assertIs(page["has_older"], True)
                else:
                    follow, expected = page["newer_cursor"], SHORT[mode][-1] + 1
                    nxt = self.page(A, "--cursor", follow, "--limit", "1", root=over)
                    self.assertEqual(self.seqs(nxt), [expected])
                    self.assertIs(page["has_newer"], True)


class WalkTests(FitCase):
    """Walks over whole logs serve every event once, in order, within the bound."""

    def check_walk(self, root, forward, expected):
        if forward:
            pages, arguments = [], ["--from", "oldest"]
            follow = "newer_cursor"
        else:
            pages, arguments = [], []
            follow = "older_cursor"
        served = []
        for _ in range(400):
            done = self.read_page(root, *arguments)
            self.assertLessEqual(size_of(done), CAP)
            page = json.loads(done.stdout)["result"]
            pages.append(page)
            served.append(self.seqs(page))
            more = page["has_newer" if forward else "has_older"]
            if not more:
                break
            arguments = ["--cursor", page[follow]]
        else:
            self.fail("the walk did not end")
        order = [seq for chunk in (served if forward else served[::-1]) for seq in chunk]
        self.assertEqual(order, expected)
        return pages

    def contiguous(self, pages, forward):
        spans = [page["span"] for page in pages if page["span"]]
        for earlier, later in zip(spans, spans[1:]):
            if forward:
                self.assertEqual(later["first_seq"], earlier["last_seq"] + 1)
            else:
                self.assertEqual(earlier["first_seq"], later["last_seq"] + 1)

    def test_a_large_event_log_is_served_once_in_both_directions(self):
        events = [event(seq, "assistant_text", "w" * 2000) for seq in range(300)]
        root = self.make("large", events)
        for forward in (True, False):
            with self.subTest(forward=forward):
                pages = self.check_walk(root, forward, list(range(300)))
                self.assertGreater(len(pages), 2)
                self.contiguous(pages, forward)

    def test_a_log_where_hidden_lines_dominate_is_served_once(self):
        events, visible = [], []
        for seq in range(700):
            if seq % 50 == 0:
                events.append(event(seq, "assistant_text", f"visible {seq}"))
                visible.append(seq)
            else:
                events.append(event(seq, "assistant_delta", "d" * 20))
        root = self.make("hidden", events)
        for forward in (True, False):
            with self.subTest(forward=forward):
                pages = self.check_walk(root, forward, visible)
                self.assertGreaterEqual(len(pages), 4, "the limit counts hidden lines")
                self.assertTrue(any(not page["events"] for page in pages) or len(pages) >= 4)
                self.contiguous(pages, forward)

    def test_hidden_lines_inside_a_page_that_has_to_shrink_are_still_counted_once(self):
        events, visible = [], []
        for seq in range(480):
            if seq % 4 == 3:
                events.append(event(seq, "assistant_delta", "d" * 30))
            else:
                events.append(event(seq, "assistant_text", "v" * 2000))
                visible.append(seq)
        root = self.make("mixed", events)
        for forward in (True, False):
            with self.subTest(forward=forward):
                pages = self.check_walk(root, forward, visible)
                self.contiguous(pages, forward)

    def test_heavy_non_ascii_text_is_sized_as_the_reference_engine_escapes_it(self):
        text = "\U0001F600中é" * 600
        events = [event(seq, "notice", text) for seq in range(90)]
        root = self.make("unicode", events)
        done = self.read_page(root, "--limit", "200")
        utf8, ascii_size = printed(done)
        self.assertLessEqual(ascii_size, CAP)
        self.assertLess(utf8, ascii_size * 0.6, "the page was sized by bytes, not by escapes")
        page = json.loads(done.stdout)["result"]
        one_more = len(json.dumps(page["events"][0])) + 2
        self.assertGreater(ascii_size + one_more, CAP, "the page is the largest that fits")
        self.assertGreater(len(page["events"]), 5)
        for forward in (True, False):
            with self.subTest(forward=forward):
                pages = self.check_walk(root, forward, list(range(90)))
                self.contiguous(pages, forward)

    def test_a_cursor_walk_never_repeats_or_skips_across_a_shrunk_page(self):
        events = [event(seq, "assistant_text", "k" * 2000) for seq in range(260)]
        root = self.make("shrunk", events)
        first = self.page(A, "--limit", "200", root=root)
        self.assertLess(len(first["events"]), 200)
        self.assertEqual(self.seqs(first)[-1], 259)
        rest = self.page(A, "--cursor", first["older_cursor"], "--limit", "200", root=root)
        self.assertEqual(self.seqs(rest)[-1] + 1, self.seqs(first)[0])


class OversizedEventTests(FitCase):
    def test_an_event_that_renders_over_sixteen_kilobytes_becomes_a_stub(self):
        fields = {f"f{index}": "z" * 2001 for index in range(8)}
        body = {"product": "harflow", "format": "harflow.event", "v": 1, "seq": 0, "kind": "notice",
                "observed_at": "2026-01-10T10:00:00+00:00", "task_id": A, **fields, "g": "q"}

        def stored(extra):
            return len(json.dumps({**body, "g": "q" * extra}, separators=(",", ":"), ensure_ascii=False).encode())

        pad = 1 + 16384 - stored(1)
        self.assertEqual(stored(pad), 16384)
        fields["g"] = "q" * pad
        self.world(seeds.task(A), events={A: [event(0, "notice", **fields)]})
        document = self.good("task-events", A)
        item = document["result"]["events"][0]
        self.assertEqual(set(item) - {"product", "format", "v", "product_version"}, {
            "seq", "kind", "observed_at", "truncated", "oversized", "bytes"})
        self.assertIs(item["truncated"], True)
        self.assertIs(item["oversized"], True)
        cut = {key: value[:2000] + "..." if len(value) > 2000 else value for key, value in fields.items()}
        rendered = {**cut, "seq": 0, "kind": "notice", "observed_at": "2026-01-10T10:00:00+00:00",
                    "truncated": True, "product": "harflow", "format": "harflow.event", "v": 1,
                    "product_version": document["product_version"]}
        self.assertEqual(item["bytes"], len(json.dumps(rendered, ensure_ascii=False).encode()))
        self.assertGreater(item["bytes"], 16384)
        self.assertEqual(document["result"]["span"], {"first_seq": 0, "last_seq": 0})

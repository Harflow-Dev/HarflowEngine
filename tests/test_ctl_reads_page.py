"""Black-box tests of `task-list-page`: paging, cursors, explicit ids, successors and the size bound."""

import base64
import json
import os

import seed_support as seeds
from reads_support import MISSING, ReadsCase, day
from seed_support import A, B, C, D, E

PAGE_BYTES = 128 * 1024


def encode(value, spaces=0):
    raw = json.dumps(value, separators=(",", ":")) + " " * spaces
    return base64.urlsafe_b64encode(raw.encode("utf-8")).decode("ascii").rstrip("=")


def decode(cursor):
    return json.loads(base64.urlsafe_b64decode(cursor + "=" * (-len(cursor) % 4)))


class CursorTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(*seeds.world()["tasks"])

    def first(self, *args, **options):
        return self.good("task-list-page", "--limit", "2", *args, **options)["result"]

    def test_pages_cover_the_history_once_in_newest_first_order(self):
        seen, cursor = [], None
        for expected in ([E, D], [C, B], [A]):
            extra = ["--cursor", cursor] if cursor else []
            page = self.good("task-list-page", "--limit", "2", *extra)["result"]
            self.assertEqual(self.ids(page["items"]), expected)
            self.assertIs(page["has_more"], expected != [A])
            seen += self.ids(page["items"])
            cursor = page["next_cursor"]
        self.assertIsNone(cursor)
        self.assertEqual(seen, [E, D, C, B, A])

    def test_the_cursor_has_the_documented_opaque_form(self):
        cursor = self.first()["next_cursor"]
        self.assertLessEqual(len(cursor), 2048)
        self.assertNotIn("=", cursor)
        body = decode(cursor)
        self.assertEqual(set(body), {"v", "scope", "stamp", "id"})
        self.assertEqual(body["v"], 1)
        self.assertEqual(body["id"], D)
        self.assertRegex(body["scope"], r"^[0-9a-f]{64}$")

    def test_the_cursor_survives_later_tasks_and_a_changed_limit(self):
        cursor = self.first()["next_cursor"]
        self.add("12345678-0000-4000-8000-0000000000aa", started_at=day(25), finished_at=day(25))
        later = self.good("task-list-page", "--limit", "3", "--cursor", cursor)["result"]
        self.assertEqual(self.ids(later["items"]), [C, B, A])
        self.assertIs(later["has_more"], False)

    def test_the_cursor_is_bound_to_the_filters(self):
        cursor = self.first()["next_cursor"]
        for label, extra in (("active-only", ["--active-only"]), ("session", ["--session-id", "x"]),
                             ("predecessor", ["--predecessor-task-id", A])):
            with self.subTest(binding=label):
                error = self.bad("task-list-page", "--limit", "2", "--cursor", cursor, *extra)
                self.assertEqual(error["code"], "invalid_params")
        session = self.first("--session-id", "session-" + D[:8])
        self.assertIsNone(session["next_cursor"])

    def test_two_identically_seeded_roots_reject_each_others_cursors(self):
        other = os.path.join(self.base, "other")
        self.seed("seed_world", {"world": seeds.world()}, root=other)
        mine = self.first()["next_cursor"]
        theirs = self.good("task-list-page", "--limit", "2", root=other)["result"]["next_cursor"]
        self.assertNotEqual(decode(mine)["scope"], decode(theirs)["scope"])
        self.bad("task-list-page", "--limit", "2", "--cursor", theirs, code="invalid_params")
        self.bad("task-list-page", "--limit", "2", "--cursor", mine, code="invalid_params", root=other)
        self.good("task-list-page", "--limit", "2", "--cursor", theirs, root=other)

    def test_cursors_over_two_thousand_and_forty_eight_characters_are_rejected(self):
        body = decode(self.first()["next_cursor"])
        fits = next(n for n in range(4000) if len(encode(body, n)) == 2048)
        over = next(n for n in range(4000) if len(encode(body, n)) > 2048)
        self.good("task-list-page", "--cursor", encode(body, fits))
        self.bad("task-list-page", "--cursor", encode(body, over), code="invalid_params")
        self.bad("task-list-page", "--cursor", encode(body, 3000), code="invalid_params")

    def test_malformed_cursors_are_invalid_params(self):
        body = decode(self.first()["next_cursor"])
        variants = {
            "text": "bogus", "empty": "", "not json": encode("x"), "list": encode([1]),
            "version": encode({**body, "v": 2}), "extra key": encode({**body, "x": 1}),
            "missing key": encode({k: v for k, v in body.items() if k != "stamp"}),
            "short scope": encode({**body, "scope": body["scope"][:10]}),
            "non hex scope": encode({**body, "scope": "z" * 64}),
            "bad id": encode({**body, "id": "bad/id"}), "float stamp": encode({**body, "stamp": 1.5}),
            "foreign scope": encode({**body, "scope": "0" * 64}),
        }
        for label, value in variants.items():
            with self.subTest(cursor=label):
                self.bad("task-list-page", "--cursor", value, code="invalid_params")


class SuccessorTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.kids = ["%08d-0000-4000-8000-000000000001" % n for n in range(1, 5)]
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)))
        for n, kid in enumerate(self.kids, start=1):
            self.add(kid, started_at=day(12 + n), finished_at=day(12 + n), continuation_of=A)

    def test_successors_page_newest_first_with_scope_fields(self):
        page = self.good("task-list-page", "--predecessor-task-id", A[:8], "--limit", "3")["result"]
        self.assertEqual(self.ids(page["items"]), self.kids[::-1][:3])
        self.assertEqual(self.ids(page["related_headers"]), [A])
        self.assertEqual(page["predecessor_task_id"], A)
        self.assertEqual(page["successor_scope"], "recorded_tasks")
        self.assertEqual(page["resolved_from_predecessor_task_id"], {"input": A[:8], "task_id": A})
        self.assertIs(page["has_more"], True)
        rest = self.good("task-list-page", "--predecessor-task-id", A, "--limit", "3",
                         "--cursor", page["next_cursor"])["result"]
        self.assertEqual(self.ids(rest["items"]), [self.kids[0]])
        self.assertNotIn("resolved_from_predecessor_task_id", rest)

    def test_no_successors_and_unknown_predecessor(self):
        page = self.good("task-list-page", "--predecessor-task-id", B)["result"]
        self.assertEqual((page["items"], page["related_headers"], page["successor_scope"]),
                         ([], [], "recorded_tasks"))
        self.bad("task-list-page", "--predecessor-task-id", MISSING, code="unknown_task")

    def test_the_predecessor_cursor_is_bound_to_its_predecessor(self):
        page = self.good("task-list-page", "--predecessor-task-id", A, "--limit", "1")["result"]
        self.bad("task-list-page", "--predecessor-task-id", B, "--limit", "1",
                 "--cursor", page["next_cursor"], code="invalid_params")
        self.bad("task-list-page", "--limit", "1", "--cursor", page["next_cursor"], code="invalid_params")

    def test_active_only_combines_with_a_predecessor(self):
        page = self.good("task-list-page", "--predecessor-task-id", A, "--active-only")["result"]
        self.assertEqual(page["items"], [])

    def test_task_ids_with_a_predecessor_is_usage(self):
        done = self.read("task-list-page", "--predecessor-task-id", A, "--task-ids", A)
        self.assertEqual((done.returncode, done.stdout), (2, ""))


class ExplicitIdTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)),
                  seeds.task(C, started_at=day(12), finished_at=day(12), continuation_of=A))

    def test_request_order_prefix_echo_and_dropped_canonical_ids(self):
        page = self.good("task-list-page", "--task-ids", f"{C[:8]},{MISSING},{A},{C}")["result"]
        self.assertEqual(self.ids(page["items"]), [C, A])
        self.assertEqual(page["resolved_from_task_ids"], [{"input": C[:8], "task_id": C}])
        self.assertEqual(set(page), {"items", "related_headers", "next_cursor", "has_more",
                                     "bootstrap_pending", "resolved_from_task_ids"})
        self.assertEqual(self.ids(page["related_headers"]), [])

    def test_a_missing_non_canonical_id_and_invalid_ids(self):
        error = self.bad("task-list-page", "--task-ids", f"{A},not-a-real-one-1", code="unknown_task")
        self.assertEqual(error["data"]["reason"], "no_prefix_match")
        error = self.bad("task-list-page", "--task-ids", f"{A},bad/id", code="invalid_params")
        self.assertIn("bad/id", error["message"])
        self.bad("task-list-page", "--task-ids", ",".join(f"x{n}" for n in range(101)),
                 code="unknown_task")

    def test_ambiguous_prefix_is_reported(self):
        for n in (1, 2):
            self.add("abababab-%04d-4000-8000-000000000001" % n)
        self.bad("task-list-page", "--task-ids", "abababab", code="ambiguous_task_id")

    def test_related_headers_walk_continuations_from_the_page_rows(self):
        page = self.good("task-list-page", "--task-ids", C)["result"]
        self.assertEqual((self.ids(page["items"]), self.ids(page["related_headers"])), ([C], [A]))
        plain = self.good("task-list-page", "--limit", "1")["result"]
        self.assertEqual((self.ids(plain["items"]), self.ids(plain["related_headers"])), ([C], [A]))
        empty = self.good("task-list-page", "--session-id", "nothing")["result"]
        self.assertEqual(empty["related_headers"], [])


class SizeBoundTests(ReadsCase):
    """Items are bounded headers: long values are cut, so the input must hold the page size
    in many keys that survive the cut (protected ids of 128 characters, other text of 200)."""

    COUNT = 100

    def setUp(self):
        super().setUp()
        self.init()
        for n in range(self.COUNT):
            stamp = "2026-03-01T10:%02d:%02d+00:00" % (n // 60, n % 60)
            self.add("%08d-0000-4000-8000-000000000001" % n, started_at=stamp, finished_at=stamp,
                     title="t" * 300, group="g" * 300, model="m" * 300, repo_path="/" + "r" * 300,
                     session_id="s" * 250)

    def test_values_are_cut_by_the_header_bound_but_status_keeps_them(self):
        item = self.good("task-list-page", "--limit", "1")["result"]["items"][0]
        self.assertEqual(len(item["title"]), 200)
        self.assertEqual(len(item["group"]), 200)
        self.assertEqual(len(item["session_id"]), 128)
        full = self.good("status", item["task_id"])["task"]
        self.assertEqual((len(full["title"]), len(full["session_id"])), (300, 250))

    def test_a_page_stops_at_128_kib_with_a_cursor_and_the_walk_covers_every_task(self):
        seen, cursor, pages = [], None, 0
        while True:
            extra = ["--cursor", cursor] if cursor else []
            result = self.good("task-list-page", *extra)["result"]
            pages += 1
            size = len(json.dumps(result["items"]).encode("utf-8"))
            self.assertLessEqual(size, PAGE_BYTES, "serialized items exceed the page bound")
            self.assertTrue(result["items"])
            seen += self.ids(result["items"])
            cursor = result["next_cursor"]
            self.assertEqual(cursor is not None, result["has_more"])
            if cursor is None:
                break
            self.assertGreater(size, PAGE_BYTES - 4096, "a continuing page is filled")
        self.assertGreater(pages, 1, "the input must reach the page bound")
        self.assertEqual(seen, ["%08d-0000-4000-8000-000000000001" % n for n in range(self.COUNT)][::-1])
        self.assertEqual(len(seen), len(set(seen)))

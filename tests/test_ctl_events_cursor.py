"""Black-box tests of the `task-events` cursors: this engine's own, the reference engine's, the
filter key and the task digest, and a stored row that does not decode.

Foreign cursors are built in Python from the reference shape (event_pages.py of HarflowMCP
a426c5a). The order under test is: well-formedness, then task, then filter, then expiry.
"""

import json

import seed_support as seeds
from history_support import (
    FILTER_ARGS, FILTER_DIGEST, PY_GENERATION, TASK_DIGEST, HistoryCase, decode, encode,
    python_cursor, python_value, sha256_text,
)
from reads_support import MISSING
from seed_support import A, B

DEFAULT = FILTER_DIGEST["default"]
RELOAD = "; reload the newest page"
INTEGERS = ("dev", "ino", "boundary")
DIGESTS = ("task", "path", "anchor", "head", "filter")


class CursorCase(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def invalid(self, cursor, *args, task=A):
        error = self.bad("task-events", task, "--cursor", cursor, *args, code="event_cursor_invalid")
        self.assertEqual(error["data"], {"reload": "newest"})
        self.assertTrue(error["message"].endswith(RELOAD), error["message"])
        return error

    def expired(self, cursor, *args, task=A):
        error = self.bad("task-events", task, "--cursor", cursor, *args, code="event_cursor_expired")
        self.assertEqual(error["data"], {"reload": "newest"})
        self.assertEqual(error["message"], "Event cursor generation is no longer served" + RELOAD)
        return error

    def foreign(self, **overrides):
        return python_cursor(A, DEFAULT, **overrides)


class ForeignCursorShapeTests(CursorCase):
    def test_a_fully_matching_foreign_cursor_is_expired_and_the_control_for_every_variation(self):
        self.expired(self.foreign())

    def test_the_expected_digests_are_the_reference_values_for_these_inputs(self):
        for task_id, literal in TASK_DIGEST.items():
            self.assertEqual(sha256_text(task_id), literal)
        import hashlib
        import json as stdlib_json
        for label, key in FILTER_DIGEST.items():
            self.assertEqual(len(key), 64, label)
        self.assertEqual(FILTER_DIGEST["assistant_delta"],
                         hashlib.sha256(stdlib_json.dumps(["assistant_delta"]).encode()).hexdigest())

    def test_an_unknown_version_is_invalid(self):
        for label, version in (("one", 1), ("three", 3), ("text", "2"), ("float", 2.0), ("bool", True),
                               ("null", None), ("large", 2 ** 64)):
            with self.subTest(version=label):
                self.invalid(self.foreign(v=version))
        value = python_value(A, DEFAULT)
        del value["v"]
        self.invalid(encode(value))

    def test_a_malformed_foreign_cursor_is_invalid(self):
        for label, value in (
            ("only gen", {"gen": PY_GENERATION}), ("empty object", {}), ("numeric gen", {"gen": 1}),
            ("only version", {"v": 2}), ("version and gen", {"v": 2, "gen": PY_GENERATION}),
            ("list", [1, 2]), ("text", "x"), ("number", 2), ("null", None), ("bool", True),
        ):
            with self.subTest(shape=label):
                self.invalid(encode(value))

    def test_text_that_is_not_a_base64_json_cursor_is_invalid(self):
        raw = "not json at all"
        import base64
        for label, text in (
            ("empty", ""), ("symbols", "***"), ("bang", "!!!!"), ("not json", base64.urlsafe_b64encode(raw.encode()).decode()),
            ("non ascii", "éééé"), ("plain", "bogus"),
        ):
            with self.subTest(text=label):
                self.invalid(text)

    def test_the_length_limit_is_two_thousand_and_forty_eight_characters(self):
        sized = {len(self.foreign(pad="p" * size)): size for size in range(2000)}
        self.assertIn(2048, sized)
        self.expired(self.foreign(pad="p" * sized[2048]))
        self.invalid(self.foreign(pad="p" * (sized[2048] + 3)))
        self.invalid(self.foreign(pad="p" * (sized[2048] + 4)))
        unpadded = {len(encode(python_value(A, DEFAULT, pad="p" * size), padded=False)): size for size in range(2000)}
        # No base64 text is 2,049 characters long, so 2,050 is the first length above the limit.
        self.assertNotIn(2049, unpadded)
        self.invalid(encode(python_value(A, DEFAULT, pad="p" * unpadded[2050]), padded=False))

    def test_each_integer_field_must_be_an_integer_below_two_to_the_sixty_four(self):
        self.expired(self.foreign(dev=2 ** 64 - 1, ino=0, boundary=0))
        bad = (("text", "5"), ("negative", -1), ("too large", 2 ** 64), ("float", 1.5), ("bool", True),
               ("null", None), ("list", []))
        for key in INTEGERS:
            for label, value in bad:
                with self.subTest(field=key, value=label):
                    self.invalid(self.foreign(**{key: value}))
            with self.subTest(field=key, value="missing"):
                content = python_value(A, DEFAULT)
                del content[key]
                self.invalid(encode(content))

    def test_the_head_length_is_zero_to_sixty_four(self):
        for value in (0, 1, 64):
            with self.subTest(hlen=value):
                self.expired(self.foreign(hlen=value))
        for label, value in (("high", 65), ("negative", -1), ("text", "64"), ("bool", True), ("float", 1.0), ("null", None)):
            with self.subTest(hlen=label):
                self.invalid(self.foreign(hlen=value))

    def test_the_five_digest_fields_are_sixty_four_characters(self):
        for key in DIGESTS:
            for label, value in (("short", "a" * 63), ("long", "a" * 65), ("empty", ""), ("number", 7), ("null", None)):
                with self.subTest(field=key, value=label):
                    self.invalid(self.foreign(**{key: value}))
            with self.subTest(field=key, value="missing"):
                content = python_value(A, DEFAULT)
                del content[key]
                self.invalid(encode(content))

    def test_the_path_anchor_and_head_are_only_checked_for_length(self):
        for key in ("path", "anchor", "head"):
            with self.subTest(field=key):
                self.expired(self.foreign(**{key: "z" * 64}))

    def test_direction_and_the_two_flags_are_checked(self):
        for value in ("older", "newer"):
            self.expired(self.foreign(dir=value))
        for value in ("up", "OLDER", 1, None, ""):
            with self.subTest(dir=value):
                self.invalid(self.foreign(dir=value))
        for key in ("resync", "skip"):
            self.expired(self.foreign(**{key: True}))
            for label, value in (("text", "false"), ("zero", 0), ("one", 1), ("null", None)):
                with self.subTest(field=key, value=label):
                    self.invalid(self.foreign(**{key: value}))

    def test_seq_is_null_or_an_integer_below_two_to_the_sixty_three(self):
        for value in (None, 0, 2 ** 63 - 1):
            with self.subTest(seq=value):
                self.expired(self.foreign(seq=value))
        for label, value in (("negative", -1), ("large", 2 ** 63), ("text", "3"), ("float", 3.5), ("bool", True)):
            with self.subTest(seq=label):
                self.invalid(self.foreign(seq=value))
        content = python_value(A, DEFAULT)
        del content["seq"]
        self.invalid(encode(content))

    def test_the_generation_is_any_text_of_at_most_thirty_two_characters(self):
        for value in ("py-events-2", "", "g" * 32, "x"):
            with self.subTest(gen=value):
                self.expired(self.foreign(gen=value))
        for label, value in (("long", "g" * 33), ("number", 5), ("null", None)):
            with self.subTest(gen=label):
                self.invalid(self.foreign(gen=value))
        content = python_value(A, DEFAULT)
        del content["gen"]
        self.invalid(encode(content))

    def test_this_engines_generation_name_needs_this_engines_shape(self):
        self.invalid(self.foreign(gen="rs-events-1"))

    def test_extra_keys_do_not_matter_to_a_foreign_cursor(self):
        self.expired(self.foreign(extra="x", more=[1, 2]))

    def test_a_padding_stripped_foreign_cursor_is_invalid_as_the_reference_decodes_it(self):
        self.invalid(encode(python_value(A, DEFAULT), padded=False))


class ForeignCursorBindingTests(CursorCase):
    def test_a_cursor_of_another_task_is_invalid(self):
        self.invalid(python_cursor(B, DEFAULT))
        self.invalid(self.foreign(task=TASK_DIGEST[B]))
        self.expired(python_cursor(B, DEFAULT), task=B)
        self.invalid(python_cursor(A, DEFAULT), task=B)

    def test_the_task_digest_is_of_the_resolved_id_not_of_the_prefix(self):
        self.expired(self.foreign(), task=A[:8])
        self.invalid(self.foreign(task=sha256_text(A[:8])), task=A[:8])

    def test_a_changed_filter_is_invalid_and_the_same_filter_is_expired(self):
        for label, arguments in FILTER_ARGS.items():
            for other, key in FILTER_DIGEST.items():
                with self.subTest(requested=label, cursor=other):
                    cursor = python_cursor(A, key)
                    if other == label:
                        self.expired(cursor, *arguments)
                    else:
                        self.invalid(cursor, *arguments)

    def test_the_filter_key_is_that_of_the_reference_for_a_kind_set_in_any_order(self):
        key = FILTER_DIGEST["assistant_text,notice"]
        for kinds in ("assistant_text,notice", "notice,assistant_text", "notice,assistant_text,notice"):
            with self.subTest(kinds=kinds):
                self.expired(python_cursor(A, key), "--kinds", kinds)
                self.invalid(python_cursor(A, FILTER_DIGEST["default"]), "--kinds", kinds)

    def test_an_all_kinds_request_matches_the_full_list_not_the_default(self):
        self.expired(python_cursor(A, FILTER_DIGEST["all"]), "--all-kinds")
        self.invalid(python_cursor(A, FILTER_DIGEST["all"]))
        self.invalid(python_cursor(A, FILTER_DIGEST["default"]), "--all-kinds")

    def test_the_shape_is_decided_before_the_binding_and_the_binding_before_expiry(self):
        self.invalid(self.foreign(v=3))
        self.invalid(self.foreign(dev="x", task=TASK_DIGEST[B]))
        self.invalid(self.foreign(task=TASK_DIGEST[B], gen="py-events-9"))
        self.invalid(self.foreign(filter=FILTER_DIGEST["all"], gen="py-events-9"))
        self.expired(self.foreign(gen="py-events-9"))

    def test_an_unknown_task_is_reported_before_the_cursor_is_read(self):
        for cursor in ("garbage", self.foreign(), python_cursor(B, DEFAULT)):
            with self.subTest(cursor=cursor[:12]):
                self.bad("task-events", MISSING, "--cursor", cursor, code="unknown_task")
                self.bad("task-events", "nomatch0123", "--cursor", cursor, code="unknown_task")

    def test_a_cursor_with_from_is_a_usage_error_before_any_authority(self):
        done = self.read("task-events", A, "--json", "--from", "newest", "--cursor", self.foreign())
        self.assertEqual(done.returncode, 2, done.stderr)
        done = self.read("task-events", A, "--json", "--from", "oldest", "--cursor", "x",
                         env={"HARFLOW_TASK_ID": "x"})
        self.assertEqual(done.returncode, 2, done.stderr)


class NativeCursorTests(CursorCase):
    def cursor(self, key="older_cursor", *args, task=A):
        return self.page(task, "--limit", "3", *args)[key]

    def test_the_cursor_has_the_documented_opaque_form(self):
        cursor = self.cursor()
        self.assertLessEqual(len(cursor), 2048)
        self.assertNotIn("=", cursor)
        body = decode(cursor)
        self.assertEqual(set(body), {"v", "gen", "store", "stream", "task", "kinds", "dir", "boundary",
                                     "resync", "seq", "skip"})
        self.assertEqual((body["v"], body["gen"], body["task"], body["dir"]), (2, "rs-events-1", A, "older"))
        self.assertEqual(body["kinds"], sorted(body["kinds"]))
        self.assertNotIn("assistant_delta", body["kinds"])

    def test_a_cursor_continues_the_page_it_came_from(self):
        first = self.page(A, "--limit", "3")
        second = self.page(A, "--cursor", first["older_cursor"], "--limit", "3")
        self.assertLess(max(self.seqs(second)), min(self.seqs(first)))

    def test_a_changed_filter_or_another_task_is_invalid(self):
        cursor = self.cursor()
        for arguments in (("--all-kinds",), ("--kinds", "notice"), ("--kinds", "assistant_text")):
            with self.subTest(arguments=arguments):
                self.invalid(cursor, *arguments)
        self.invalid(cursor, task=B)

    def test_a_store_stream_or_boundary_that_no_longer_applies_is_expired(self):
        body = decode(self.cursor())
        for key, value in (("store", "0" * 32), ("stream", "0" * 32), ("boundary", 10 ** 9)):
            with self.subTest(field=key):
                self.expired(encode({**body, key: value}, padded=False))

    def test_a_truncated_removed_or_replaced_log_expires_its_cursors(self):
        older, newer = self.cursor(), self.cursor("newer_cursor")
        self.seed("truncate_log", {"task_id": A, "keep_events": 2})
        self.expired(older)
        self.expired(newer)
        fresh = self.cursor("newer_cursor")
        self.seed("remove_log", {"task_id": A})
        self.expired(fresh)
        self.seed("append_event", {"task_id": A, **seeds.a_events()[0]})
        self.expired(fresh)

    def test_a_cursor_of_another_root_is_expired(self):
        import os
        other = os.path.join(self.base, "other")
        self.seed("seed_world", {"world": seeds.world()}, root=other)
        theirs = self.page(A, "--limit", "3", root=other)["older_cursor"]
        self.expired(theirs)

    def test_a_cursor_with_a_wrong_field_is_invalid(self):
        body = decode(self.cursor())
        removed = {key: {k: v for k, v in body.items() if k != key} for key in body}
        changed = {
            "v": 1, "gen": 5, "store": 5, "stream": None, "task": 5, "task-id": "bad id", "kinds": "all",
            "kinds-item": [1], "dir": "up", "boundary": -1, "boundary-text": "5", "resync": 0,
            "seq": "x", "skip": None,
        }
        for key in body:
            with self.subTest(removed=key):
                self.invalid(encode(removed[key], padded=False))
        for label, value in changed.items():
            key = label.split("-")[0]
            with self.subTest(changed=label):
                self.invalid(encode({**body, key: value}, padded=False))
        self.invalid(encode({**body, "extra": 1}, padded=False))


class StoredRowTests(CursorCase):
    def corrupt(self, seq):
        self.sql("UPDATE event_records SET body=? WHERE task=? AND seq=?", (b"\xff\xfe\xfd", A, seq))

    def incompatible(self, *args):
        error = self.bad("task-events", A, *args, code="state_incompatible")
        self.assertEqual(error["data"]["reason"], "infrastructure_unavailable")
        self.assertEqual(error["message"], "Task history cannot be read")
        self.assertNotIn("event_cursor", json.dumps(error))
        return error

    def test_a_corrupt_stored_row_without_a_cursor_never_produces_a_cursor_code(self):
        self.corrupt(3)
        for arguments in ((), ("--from", "oldest"), ("--all-kinds",), ("--kinds", "assistant_text")):
            with self.subTest(arguments=arguments):
                self.incompatible(*arguments)

    def test_a_corrupt_row_met_by_a_valid_cursor_is_a_store_error_too(self):
        first = self.page(A, "--limit", "2")
        self.corrupt(3)
        self.incompatible("--cursor", first["older_cursor"], "--limit", "5")

    def test_a_corrupt_row_off_the_page_does_not_affect_it(self):
        self.corrupt(0)
        page = self.page(A, "--limit", "2")
        self.assertEqual(self.seqs(page), [6, 7])

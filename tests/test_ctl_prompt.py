"""Black-box tests of `task-prompt`: every state of the integrity table, the byte bound and its
cut on a character boundary, invalid bytes, and the 4 MiB verification ceiling."""

import hashlib

import seed_support as seeds
from history_support import HistoryCase
from seed_support import A, B

CEILING = 4 * 1024 * 1024
PREVIEW_CHARS = 2000
KEYS = {"state", "text", "returned_bytes", "total_bytes", "truncated", "complete", "integrity"}


def lines(count):
    return "".join(f"line {n:05d} é\n" for n in range(count))


class TableTests(HistoryCase):
    def result(self, store, prompt, **fields):
        self.init(seeds.task(A, prompt=prompt, prompt_store=store, title="Named", **fields))
        return self.good("task-prompt", A)["result"]

    def check(self, result, **expected):
        prompt = result["prompt"]
        self.assertEqual(set(prompt), KEYS)
        for key, value in expected.items():
            self.assertEqual(prompt[key], value, key)
        self.assertEqual((result["task_id"], result["title"]), (A, "Named"))

    def test_an_inline_prompt_is_available_and_not_applicable(self):
        text = "Summarize the repo.\n"
        self.check(self.result("inline", text), state="available", integrity="not_applicable", text=text,
                   returned_bytes=len(text), total_bytes=len(text), truncated=False, complete=True)

    def test_a_non_ascii_inline_prompt_counts_bytes_not_characters(self):
        text = "héllo 中文 \U0001F600"
        self.check(self.result("inline", text), text=text, returned_bytes=len(text.encode()),
                   total_bytes=len(text.encode()), complete=True)

    def test_a_sidecar_with_a_foreign_name_is_unavailable(self):
        self.check(self.result("sidecar_foreign_name", lines(300)), state="unavailable",
                   integrity="not_applicable", text=None, returned_bytes=0, total_bytes=None,
                   truncated=False, complete=False)

    def test_a_missing_sidecar_gives_the_preview_only(self):
        prompt = lines(300)
        result = self.result("sidecar_missing", prompt)
        preview = prompt[:PREVIEW_CHARS]
        self.check(result, state="preview_only", integrity="not_applicable", text=preview,
                   returned_bytes=len(preview.encode()), total_bytes=None, truncated=False, complete=False)

    def test_a_verified_sidecar_is_served_whole(self):
        prompt = lines(300)
        self.check(self.result("sidecar_verified", prompt), state="available", integrity="verified",
                   text=prompt, returned_bytes=len(prompt.encode()), total_bytes=len(prompt.encode()),
                   truncated=False, complete=True)

    def test_a_sidecar_that_differs_from_its_digest_fails_integrity(self):
        self.check(self.result("sidecar_mismatch", lines(300)), state="integrity_failed", integrity="failed",
                   text=None, returned_bytes=0, total_bytes=None, truncated=False, complete=False)

    def test_a_sidecar_tampered_after_the_write_fails_integrity(self):
        self.init(seeds.task(A, prompt=lines(300), prompt_store="sidecar_verified", title="Named"))
        self.seed("tamper_prompt", {"task_id": A})
        self.check(self.good("task-prompt", A)["result"], state="integrity_failed", integrity="failed",
                   text=None, total_bytes=None)

    def test_a_short_prompt_stored_as_a_sidecar_is_still_read_through_its_blob(self):
        self.check(self.result("sidecar_verified", "tiny"), integrity="verified", text="tiny", total_bytes=4)


class CutTests(HistoryCase):
    def prompt(self, text, *args):
        self.init(seeds.task(A, prompt=text, title="Named"))
        return self.good("task-prompt", A, *args)["result"]["prompt"]

    def test_the_default_bound_is_sixty_five_thousand_five_hundred_and_thirty_six_bytes(self):
        fits = self.prompt("a" * 65_536)
        self.assertEqual((fits["returned_bytes"], fits["truncated"], fits["complete"]), (65_536, False, True))
        self.setUp()
        over = self.prompt("a" * 65_537)
        self.assertEqual((over["returned_bytes"], over["truncated"], over["complete"]), (65_536, True, False))
        self.assertEqual(over["total_bytes"], 65_537)

    def test_the_bound_is_a_parameter_from_one_to_one_hundred_and_thirty_one_thousand_and_seventy_two(self):
        text = "b" * 140_000
        for value, returned in (("1", 1), ("100", 100), ("131072", 131_072)):
            with self.subTest(max_bytes=value):
                self.setUp()
                prompt = self.prompt(text, "--max-bytes", value)
                self.assertEqual((prompt["returned_bytes"], prompt["truncated"], prompt["complete"]),
                                 (returned, True, False))
                self.assertEqual(prompt["text"], "b" * returned)

    def test_a_cut_never_splits_a_character(self):
        cases = (
            ("é" * 10, 1, "", 0), ("é" * 10, 3, "é", 2), ("é" * 10, 4, "éé", 4),
            ("中" * 10, 2, "", 0), ("中" * 10, 5, "中", 3), ("中" * 10, 6, "中中", 6),
            ("\U0001F600" * 10, 3, "", 0), ("\U0001F600" * 10, 7, "\U0001F600", 4),
            ("\U0001F600" * 10, 8, "\U0001F600\U0001F600", 8),
            ("aéb", 2, "a", 1), ("aéb", 3, "aé", 3),
        )
        for text, bound, expected, returned in cases:
            with self.subTest(text=text[:3], bound=bound):
                self.setUp()
                prompt = self.prompt(text, "--max-bytes", str(bound))
                self.assertEqual(prompt["text"], expected)
                self.assertEqual(prompt["returned_bytes"], returned)
                self.assertIs(prompt["truncated"], True)
                self.assertIs(prompt["complete"], False)
                self.assertEqual(prompt["total_bytes"], len(text.encode()))

    def test_a_bound_exactly_on_a_character_edge_is_not_a_cut(self):
        prompt = self.prompt("ééé", "--max-bytes", "6")
        self.assertEqual((prompt["text"], prompt["truncated"], prompt["complete"]), ("ééé", False, True))


class Sidecars(HistoryCase):
    """The port refuses large or invalid prompts, so the sidecar is rewritten in the store."""

    def rewrite(self, raw, digest_of=None):
        self.init(seeds.task(A, prompt="placeholder text", prompt_store="sidecar_verified", title="Named"))
        name = f"{A}.prompt.{hashlib.sha256(raw if digest_of is None else digest_of).hexdigest()}.txt"
        self.sql("UPDATE task_prompts SET blob=?, source_name=? WHERE task=?", (raw, name, A))


class InvalidByteTests(Sidecars):

    def test_invalid_bytes_in_a_verified_sidecar_become_replacement_characters(self):
        self.rewrite(b"ok \xff\xfe tail")
        prompt = self.good("task-prompt", A)["result"]["prompt"]
        self.assertEqual(prompt["text"], "ok �� tail")
        self.assertEqual(prompt["integrity"], "verified")
        self.assertEqual(prompt["total_bytes"], len(b"ok \xff\xfe tail"))
        self.assertEqual(prompt["returned_bytes"], len("ok �� tail".encode()))
        self.assertIs(prompt["complete"], False, "the decoded text is not the stored bytes")
        self.assertIs(prompt["truncated"], False)

    def test_a_replacement_character_longer_than_its_byte_is_cut_again_on_a_boundary(self):
        self.rewrite(b"ok \xff\xfe tail")
        prompt = self.good("task-prompt", A, "--max-bytes", "5")["result"]["prompt"]
        self.assertEqual(prompt["text"], "ok ")
        self.assertEqual(prompt["returned_bytes"], 3)
        self.assertIs(prompt["truncated"], True)

    def test_a_character_cut_off_by_the_byte_prefix_is_dropped_not_replaced(self):
        self.rewrite("abécd".encode())
        prompt = self.good("task-prompt", A, "--max-bytes", "3")["result"]["prompt"]
        self.assertEqual((prompt["text"], prompt["returned_bytes"]), ("ab", 2))


class CeilingTests(Sidecars):
    """A sidecar up to 4 MiB is hashed whole; a larger one is served as an unverified prefix."""

    PATTERN = "0123456789abcdef"

    def sidecar(self, size, digest_of=None):
        self.rewrite((self.PATTERN * (size // 16 + 1))[:size].encode(), digest_of)
        return self.good("task-prompt", A)["result"]["prompt"]

    def test_exactly_four_mebibytes_is_verified(self):
        prompt = self.sidecar(CEILING)
        self.assertEqual((prompt["integrity"], prompt["state"], prompt["total_bytes"]), ("verified", "available", CEILING))
        self.assertEqual((prompt["returned_bytes"], prompt["truncated"], prompt["complete"]), (65_536, True, False))
        self.assertEqual(prompt["text"], self.PATTERN * 4096)

    def test_one_byte_over_four_mebibytes_is_unverified(self):
        prompt = self.sidecar(CEILING + 1)
        self.assertEqual((prompt["integrity"], prompt["state"], prompt["total_bytes"]), ("unverified", "available", CEILING + 1))
        self.assertEqual((prompt["returned_bytes"], prompt["truncated"], prompt["complete"]), (65_536, True, False))
        self.assertEqual(prompt["text"], self.PATTERN * 4096)

    def test_a_mismatch_is_caught_up_to_the_ceiling_and_not_beyond(self):
        failed = self.sidecar(CEILING, digest_of=b"another body")
        self.assertEqual((failed["state"], failed["integrity"], failed["text"]), ("integrity_failed", "failed", None))
        self.setUp()
        over = self.sidecar(CEILING + 1, digest_of=b"another body")
        self.assertEqual((over["state"], over["integrity"], over["total_bytes"]), ("available", "unverified", CEILING + 1))
        self.assertEqual(over["returned_bytes"], 65_536)

    def test_an_unverified_prefix_is_bounded_by_the_requested_bytes(self):
        self.sidecar(CEILING + 1)
        for value, returned in (("1", 1), ("1000", 1000)):
            with self.subTest(max_bytes=value):
                prompt = self.good("task-prompt", A, "--max-bytes", value)["result"]["prompt"]
                self.assertEqual((prompt["returned_bytes"], prompt["integrity"]), (returned, "unverified"))

    def test_invalid_bytes_in_an_unverified_prefix_are_replaced_and_recut(self):
        self.rewrite(b"ab\xff\xfecd" + b"x" * (CEILING + 1 - 6))
        prompt = self.good("task-prompt", A, "--max-bytes", "10")["result"]["prompt"]
        self.assertEqual(prompt["text"], "ab\ufffd\ufffdcd")
        self.assertEqual((prompt["returned_bytes"], prompt["truncated"], prompt["complete"]), (10, True, False))
        self.assertEqual((prompt["integrity"], prompt["total_bytes"]), ("unverified", CEILING + 1))


class DigestTests(HistoryCase):
    def test_a_prompt_for_another_task_does_not_leak_between_tasks(self):
        self.init(seeds.task(A, prompt="first prompt"), seeds.task(B, prompt="second prompt",
                                                                  started_at="2026-01-11T10:00:00+00:00"))
        self.assertEqual(self.good("task-prompt", A)["result"]["prompt"]["text"], "first prompt")
        self.assertEqual(self.good("task-prompt", B)["result"]["prompt"]["text"], "second prompt")

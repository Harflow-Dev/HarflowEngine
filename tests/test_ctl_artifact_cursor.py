"""Artifact cursors in several base64 spellings, with the verdicts of the reference.

The reference decodes with the non-validating `base64.urlsafe_b64decode` (artifacts.py of
HarflowMCP a426c5a, `get`): it needs the data to end on a complete quad, throws away characters
outside the alphabet and stops at padding. The verdicts below were computed with the local
python3 3.13.5 on the reference's own checks for a cursor `[task, revision, 2]` over five items,
so they are literals and the engine under test is never their source. A valid cursor continues
at item 2; a stale one is an `artifact_error`. The standard alphabet is not tabled: a cursor
that is valid JSON of this shape never encodes to the 62nd or 63rd character, so the two
alphabets spell it alike.
"""

import base64
import json

from history_support import ArtifactCase, item
from seed_support import A

VERDICTS = {'pad2/padded': 'valid',
 'pad2/padding stripped': 'stale',
 'pad2/junk inside': 'valid',
 'pad2/space inside': 'valid',
 'pad2/newline inside': 'valid',
 'pad2/equals inside': 'valid',
 'pad2/trailing junk': 'valid',
 'pad2/trailing quad': 'valid',
 'pad2/trailing equals': 'valid',
 'pad2/trailing double equals': 'valid',
 'pad2/trailing newline': 'valid',
 'pad2/leading space': 'valid',
 'pad2/non-ascii': 'stale',
 'pad2/empty': 'stale',
 'pad1/padded': 'valid',
 'pad1/padding stripped': 'stale',
 'pad1/junk inside': 'valid',
 'pad1/space inside': 'valid',
 'pad1/newline inside': 'valid',
 'pad1/equals inside': 'valid',
 'pad1/trailing junk': 'valid',
 'pad1/trailing quad': 'valid',
 'pad1/trailing equals': 'valid',
 'pad1/trailing double equals': 'valid',
 'pad1/trailing newline': 'valid',
 'pad1/leading space': 'valid',
 'pad1/non-ascii': 'stale',
 'pad1/empty': 'stale',
 'pad0/padded': 'valid',
 'pad0/padding stripped': 'valid',
 'pad0/junk inside': 'valid',
 'pad0/space inside': 'valid',
 'pad0/newline inside': 'valid',
 'pad0/equals inside': 'valid',
 'pad0/trailing junk': 'valid',
 'pad0/trailing quad': 'stale',
 'pad0/trailing equals': 'valid',
 'pad0/trailing double equals': 'valid',
 'pad0/trailing newline': 'valid',
 'pad0/leading space': 'valid',
 'pad0/non-ascii': 'stale',
 'pad0/empty': 'stale'}


def variants(s):
    return {
        "padded": s,
        "padding stripped": s.rstrip("="),
        "junk inside": s[:10] + "!" + s[10:],
        "space inside": s[:10] + " " + s[10:],
        "newline inside": s[:10] + "\n" + s[10:],
        "equals inside": s[:10] + "=" + s[10:],
        "trailing junk": s + "!!",
        "trailing quad": s + "AAAA",
        "trailing equals": s + "=",
        "trailing double equals": s + "==",
        "trailing newline": s + "\n",
        "leading space": " " + s,
        "non-ascii": s[:8] + "\u00e9" + s[8:],
        "empty": "",
    }


class CursorSpellingTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(*(item(f"a{n}", f"f{n}.txt") for n in range(5)))
        first = self.listed("--limit", "2")["next_cursor"]
        self.revision = json.loads(base64.urlsafe_b64decode(first))[1]

    def test_every_spelling_gets_the_verdict_of_the_reference_decoder(self):
        seen = set()
        for spaces in range(3):
            raw = json.dumps([A, self.revision, 2]).replace("]", " " * spaces + "]").encode()
            encoded = base64.urlsafe_b64encode(raw).decode()
            for name, cursor in variants(encoded).items():
                key = f"pad{encoded.count('=')}/{name}"
                seen.add(key)
                with self.subTest(cursor=key):
                    if VERDICTS[key] == "valid":
                        self.assertEqual(self.listed("--limit", "2", "--cursor", cursor)["items"][0]["artifact_id"], "a2")
                    else:
                        error = self.bad("task-artifacts", A, "--cursor", cursor, code="artifact_error")
                        self.assertEqual(error["message"], "Invalid or stale artifact cursor")
        self.assertEqual(seen, set(VERDICTS))

    def test_the_cursor_this_engine_issues_is_padded_like_the_reference_encoder(self):
        cursor = self.listed("--limit", "2")["next_cursor"]
        raw = base64.urlsafe_b64decode(cursor)
        self.assertEqual(cursor, base64.urlsafe_b64encode(raw).decode())
        self.assertEqual(json.loads(raw), [A, self.revision, 2])
        # The reference writes json.dumps default separators.
        self.assertEqual(raw.decode(), json.dumps([A, self.revision, 2]))

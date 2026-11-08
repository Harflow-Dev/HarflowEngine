"""Every wire document is written exactly as python3 3.13.5 `json.dumps` writes it with its
defaults (separators ", " and ": ", `ensure_ascii`), with sorted keys: one line for `--json`,
`indent=2` for the bare result, and the error document. The oracle is the stdlib: the document the
engine printed is parsed and written again, and the bytes must be equal.
"""

import json

import seed_support as seeds
from feeds_support import WIDE, FeedsCase, settled

# e-acute, an emoji (a surrogate pair), a control character, DEL, a quote and a backslash.
TITLE = "café \U0001F600 \x01 \x7f \" \\ end"
ESCAPES = ("\\u00e9", "\\ud83d\\ude00", "\\u0001", "\\u007f", '\\"', "\\\\")


class OutputFormTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("o1", 0, title=TITLE), settled("o2", 10, title="x\ty\nz"))

    def assert_python_bytes(self, stdout, indent=None):
        document = json.loads(stdout)
        self.assertEqual(stdout, json.dumps(document, sort_keys=True, indent=indent) + "\n")
        self.assertTrue(stdout.isascii())
        return document

    def test_the_line_form_of_both_reads_is_json_dumps_with_its_defaults(self):
        for args in (("interaction-events",), ("statistics-page", "--since", WIDE[0], "--before", WIDE[1])):
            with self.subTest(command=args[0]):
                done = self.read(*args, "--json")
                self.assertEqual((done.returncode, done.stderr), (0, ""))
                self.assert_python_bytes(done.stdout)
                for escaped in ESCAPES:
                    self.assertIn(escaped, done.stdout)

    def test_the_pretty_form_of_both_reads_is_json_dumps_with_indent_two(self):
        for args in (("interaction-events",), ("statistics-page", "--since", WIDE[0], "--before", WIDE[1])):
            with self.subTest(command=args[0]):
                done = self.read(*args)
                self.assertEqual((done.returncode, done.stderr), (0, ""))
                document = self.assert_python_bytes(done.stdout, indent=2)
                self.assertTrue(done.stdout.startswith("{\n  "))
                self.assertNotIn("product", document)
                for escaped in ESCAPES:
                    self.assertIn(escaped, done.stdout)

    def test_the_title_survives_the_round_trip_in_both_documents(self):
        events = self.feed()["events"]
        self.assertEqual(events[0]["title"], TITLE)
        self.assertEqual(events[1]["title"], "x\ty\nz")
        items = self.stats()["items"]
        self.assertEqual([item["title"] for item in items], [TITLE, "x\ty\nz"])

    def test_the_error_document_is_json_dumps_with_its_defaults(self):
        for args in (("interaction-events", "--limit", "0"),
                     ("statistics-page", "--since", "nonsense", "--before", "nonsense"),
                     ("recovery-activity", "bad/id"), ("interaction-events", "--cursor", "é中\"\\")):
            with self.subTest(args=args):
                done = self.read(*args, "--json")
                self.assertEqual(done.returncode, 1)
                document = self.assert_python_bytes(done.stdout)
                self.assertIn("error", document)

    def test_the_recovery_envelope_is_written_the_same_way(self):
        self.add(seeds.A)
        done = self.read("recovery-activity", seeds.A)
        self.assertEqual(done.returncode, 0)
        self.assert_python_bytes(done.stdout)

    def test_an_error_message_with_non_ascii_text_is_escaped(self):
        done = self.read("task-events", "café\U0001F600", "--json")
        self.assertEqual(done.returncode, 1)
        document = self.assert_python_bytes(done.stdout)
        self.assertIn("\\u00e9", done.stdout)
        self.assertIn("\\ud83d\\ude00", done.stdout)
        self.assertIn("café", document["error"]["message"])

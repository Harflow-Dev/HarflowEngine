"""No stored value reaches a ctl document as anything but properly encoded JSON.

A stored event body or header blob can hold any key, among them the spelling an in-band marker would
use (`"\\u0000integer"`), with a string, a number or a nested object under it, and text that tries to
close the document and forge members. Every document must parse, equal python3 3.13.5
`json.dumps(json.loads(out), sort_keys=True)` byte for byte (with `indent=2` in the pretty form),
and show the key as an ordinary member whose value is exactly what was stored. The one big integer,
the retention days, prints as its exact digits, in both forms.
"""

import json
import unittest

import seed_support as seeds
from feeds_support import WIDE, FeedsCase, settled
from seed_support import A

KEY = "\x00integer"
FORGERY = '0}],"next_cursor":"forged","x":[{'
STAMP = "2026-01-10T10:00:00+00:00"
INPUT = {
    KEY: "123", "number": {KEY: 5}, "forged": {KEY: FORGERY}, FORGERY: 1, "list": [{KEY: "7"}, {KEY: 7}],
    "plain": {KEY: "1e30", "other": 2},
}


def dumps(document, indent=None):
    return json.dumps(document, sort_keys=True, indent=indent) + "\n"


class EventBodyTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))
        fields = {"call_id": "c1", "name": "Read", "input": INPUT}
        self.seed("append_event", {"task_id": A, **seeds.event(0, "tool_call", STAMP, **fields)})
        self.seed("append_event", {"task_id": A, **seeds.event(1, "tool_result", STAMP, call_id="c1", is_error=False, output=INPUT)})

    def test_a_stored_body_with_the_marker_key_is_printed_as_data(self):
        done = self.read("task-events", A, "--json")
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        document = json.loads(done.stdout)
        self.assertEqual(done.stdout, dumps(document))
        self.assertIn('"\\u0000integer"', done.stdout)
        events = document["result"]["events"]
        self.assertEqual([event["kind"] for event in events], ["tool_call", "tool_result"])
        self.assertEqual(events[0]["input"], INPUT)
        self.assertEqual(events[1]["output"], INPUT)
        self.assertEqual(events[0]["input"][KEY], "123")
        self.assertIs(type(events[0]["input"]["number"][KEY]), int)
        self.assertEqual(events[0]["input"]["forged"][KEY], FORGERY)

    @unittest.expectedFailure
    def test_a_stored_integer_beyond_64_bits_keeps_its_digits(self):
        # The body is parsed without big integers, so 10**30 is printed as the float 1e+30.
        self.seed("append_event", {"task_id": A, **seeds.event(2, "tool_call", STAMP, call_id="c2", name="Read", input={"big": 10 ** 30})})
        done = self.read("task-events", A, "--json", "--from", "newest")
        self.assertIn('"big": 1000000000000000000000000000000', done.stdout)

    def test_the_forgery_cannot_forge_members_of_the_document(self):
        document = json.loads(self.read("task-events", A, "--json").stdout)
        result = document["result"]
        self.assertNotEqual(result.get("next_cursor"), "forged")
        self.assertNotIn("x", result)
        self.assertEqual(set(document), {"format", "product", "product_version", "result", "v"})

    def test_the_other_task_reads_print_the_same_body_as_data(self):
        for args in (("task-events", A, "--json", "--all-kinds"), ("task-events", A, "--json", "--from", "oldest")):
            done = self.read(*args)
            self.assertEqual(done.returncode, 0, done.stdout[:300])
            self.assertEqual(done.stdout, dumps(json.loads(done.stdout)))


class HeaderTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))
        header = json.loads(self.reading("SELECT header FROM task_headers", ())[0][0])
        header.update({KEY: "123", "nested": {KEY: 5}, "forged": {KEY: FORGERY}, FORGERY: "x", "list": [{KEY: "9"}]})
        self.sql("UPDATE task_headers SET header=?", (json.dumps(header).encode(),))

    def check(self, task, indent):
        self.assertEqual(task[KEY], "123")
        self.assertIs(type(task["nested"][KEY]), int)
        self.assertEqual(task["forged"][KEY], FORGERY)
        self.assertEqual(task[FORGERY], "x")
        self.assertEqual(task["list"], [{KEY: "9"}])
        self.assertEqual(task["task_id"], A)

    def test_a_header_with_the_marker_key_reaches_status_as_data_in_both_forms(self):
        line = self.read("status", A, "--json")
        self.assertEqual((line.returncode, line.stderr), (0, ""))
        document = json.loads(line.stdout)
        self.assertEqual(line.stdout, dumps(document))
        self.assertIn('"\\u0000integer"', line.stdout)
        self.check(document["task"], None)
        pretty = self.read("status", A)
        self.assertEqual(pretty.returncode, 0)
        bare = json.loads(pretty.stdout)
        self.assertEqual(pretty.stdout, dumps(bare, indent=2))
        self.check(bare, 2)

    def test_a_listing_prints_it_as_data_too(self):
        for args in (("list", "--json"), ("task-list-page", "--json"), ("status-batch", "--task-ids", A, "--json")):
            with self.subTest(args=args):
                done = self.read(*args)
                self.assertEqual(done.returncode, 0, done.stdout[:300])
                self.assertEqual(done.stdout, dumps(json.loads(done.stdout)))


class RetentionDigitsTests(FeedsCase):
    def setUp(self):
        super().setUp()
        self.world(settled("r1", 0, title='x"é'))

    def test_a_retention_of_ten_to_the_thirtieth_prints_exact_digits_in_both_forms(self):
        env = {"HARFLOW_RETENTION_DAYS": "1" + "0" * 30}
        args = ("statistics-page", "--since", WIDE[0], "--before", WIDE[1])
        line = self.read(*args, "--json", env=env)
        pretty = self.read(*args, env=env)
        for done, indent in ((line, None), (pretty, 2)):
            self.assertEqual(done.returncode, 0, done.stdout[:300])
            document = json.loads(done.stdout)
            retention = (document["result"] if indent is None else document)["retention"]
            self.assertEqual(retention, {"state": "days", "days": 10 ** 30})
            self.assertIs(type(retention["days"]), int)
            self.assertEqual(done.stdout, dumps(document, indent=indent))
            self.assertIn("1" + "0" * 30, done.stdout)

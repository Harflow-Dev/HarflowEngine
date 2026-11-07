"""Black-box tests of the `outcome` and `failure_diagnostic` projections of `status --brief`
against values computed by brief_status() of the reference (see brief_expected.py)."""

import json
import sqlite3
import unittest

import seed_support as seeds
from brief_expected import CASES
from history_support import HistoryCase
from seed_support import A


class ProjectionTests(HistoryCase):
    def brief(self, case):
        payload = CASES[case]["payload"]
        self.world(seeds.task(A))
        db = sqlite3.connect(self.state + "/store.sqlite3")
        try:
            blob = db.execute("SELECT status FROM task_headers WHERE id=?", (A,)).fetchone()[0]
        finally:
            db.close()
        status = {**json.loads(blob), **payload}
        self.sql("UPDATE task_headers SET status=? WHERE id=?", (json.dumps(status, ensure_ascii=False).encode(), A))
        return self.good("status", A, "--brief")["task"]

    def check(self, case, expected_outcome=None, expected_diagnostic=None):
        task = self.brief(case)
        expected = CASES[case]
        self.assertEqual(task.get("outcome"), expected["outcome"] if expected_outcome is None else expected_outcome)
        self.assertEqual(task.get("failure_diagnostic"),
                         expected["failure_diagnostic"] if expected_diagnostic is None else expected_diagnostic)
        self.assertEqual(task["truncated_fields"], expected["truncated_fields"])
        return task

    def test_the_compact_rules_match_the_reference(self):
        self.check("compact")

    def test_a_small_outcome_still_gains_the_attribution_and_the_flag(self):
        self.check("small")

    def test_an_existing_truncation_flag_is_kept(self):
        self.check("flag")

    def test_a_number_diagnostic_is_written_as_text_and_both_are_reported_cut(self):
        task = self.brief("scalars")
        self.assertEqual(task["failure_diagnostic"], CASES["scalars"]["failure_diagnostic"])
        self.assertEqual(len(task["outcome"].encode()), 160)
        self.assertEqual(task["truncated_fields"], CASES["scalars"]["truncated_fields"])

    def test_a_string_outcome_is_cut_without_quotes_as_the_reference_does(self):
        """The reference takes `str()` of a non-object, which for a string is the string itself,
        so the text gains no quotation marks."""
        self.check("scalars")

    def test_a_task_without_either_has_neither_key(self):
        self.world(seeds.task(A))
        task = self.good("status", A, "--brief")["task"]
        self.assertNotIn("outcome", task)
        self.assertNotIn("failure_diagnostic", task)
        self.assertEqual(task["truncated_fields"], [])

    def test_the_full_status_keeps_the_stored_values_whole(self):
        self.brief("compact")
        full = self.good("status", A)["task"]
        self.assertEqual(full["outcome"], CASES["compact"]["payload"]["outcome"])
        self.assertNotIn("attribution", full["outcome"])

    def test_members_are_kept_in_sorted_order_where_the_reference_keeps_insertion_order(self):
        """Known difference: a stored object is read into a sorted map, so the four members kept
        are the alphabetically first four. The reference keeps the first four inserted."""
        task = self.brief("unsorted")
        self.assertEqual(task["outcome"]["local_upstream"], {"v": 5, "w": 4, "x": 3, "y": 2})
        self.assertEqual(CASES["unsorted"]["outcome"]["local_upstream"], {"z": 1, "y": 2, "x": 3, "w": 4})
        self.assertEqual(task["truncated_fields"], CASES["unsorted"]["truncated_fields"])

    @unittest.expectedFailure
    def test_a_list_diagnostic_is_cut_as_the_reference_writes_it(self):
        """The reference takes `str()` of a non-object, so a list reads `[1, 2]`; this engine
        writes its compact JSON, `[1,2]`."""
        self.check("list")

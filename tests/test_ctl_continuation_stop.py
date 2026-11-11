"""Black-box tests of the `continuation_stop` object of `status`, `status --brief` and
`status-batch`, over roots built with hf-fixture-seed and read with the built harflow-ctl.

The oracle is Python's own json, datetime and re: nothing is shared with the Rust projection.
"""

import datetime
import json
import re
import unittest
from collections import OrderedDict

import seed_support as seeds
from reads_support import ReadsCase, day
from seed_support import A, B, C, D, E

KEYS = ["cancelled_task_id", "chain_root_task_id", "observed_at", "reason", "state"]
STAMP = re.compile(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{6})?\+00:00$")
BAD_ROOT = "not a valid id!"
F = "ffffffff-ffff-4fff-8fff-ffffffffffff"


def member(text):
    """The continuation_stop member of a document, with its key order preserved."""
    found = []

    def pairs(items):
        mapping = OrderedDict(items)
        if "continuation_stop" in mapping:
            found.append(mapping["continuation_stop"])
        return mapping

    json.loads(text, object_pairs_hook=pairs)
    return found


class StopCase(ReadsCase):
    def setUp(self):
        super().setUp()
        # A is a chain root, B and C continue it, D stands alone, E stores an invalid root.
        self.init(
            seeds.task(A),
            seeds.task(B, started_at=day(11), finished_at=day(11), continuation_of=A),
            seeds.task(C, started_at=day(12), finished_at=day(12), continuation_of=B),
            seeds.task(D, started_at=day(13), finished_at=day(13)),
            seeds.task(E, started_at=day(14), finished_at=day(14), continuation_of=A, chain_root=BAD_ROOT),
        )

    def stop(self, root, mode, **attribution):
        spec = {"chain_root": root, "mode": mode}
        if attribution:
            spec["attribution"] = attribution
        self.seed("stop_chain", {"spec": spec})

    def stop_of(self, task_id, *flags):
        done = self.read("status", task_id, "--json", *flags)
        self.assertEqual((done.returncode, done.stderr), (0, ""), done.stdout)
        (found,) = member(done.stdout)
        self.assertEqual(list(found), KEYS)
        self.assertRegex(found["observed_at"], STAMP)
        return found

    def check_stamp(self, value):
        moment = datetime.datetime.fromisoformat(value)
        age = datetime.datetime.now(datetime.timezone.utc) - moment
        self.assertLess(abs(age.total_seconds()), 600, value)

    def assertStop(self, found, state, root, reason=None, cancelled=None):
        self.assertEqual(
            {key: found[key] for key in KEYS if key != "observed_at"},
            {"state": state, "chain_root_task_id": root, "reason": reason, "cancelled_task_id": cancelled},
        )
        self.check_stamp(found["observed_at"])


class StatusStopTests(StopCase):
    def test_a_chain_without_a_marker_is_not_stopped(self):
        self.assertStop(self.stop_of(D), "not_stopped", D)
        self.assertStop(self.stop_of(B), "not_stopped", A)

    def test_a_marker_with_a_valid_attribution_stops_the_chain(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=C)
        self.assertStop(self.stop_of(A), "stopped", A, "chain_cancelled", C)

    def test_unusable_attributions_stop_the_chain_with_a_null_cancelled_task(self):
        cases = (
            dict(kind="invalid", cancelled_task_id="not an id!"),
            dict(kind="invalid", cancelled_task_id=""),
            dict(kind="invalid", cancelled_task_id="a" * 65),
            dict(kind="malformed", cancelled_task_id=None),
            dict(kind="oversize", cancelled_task_id=C),
        )
        for attribution in cases:
            with self.subTest(attribution=attribution):
                self.stop(D, "absent")
                self.stop(D, "marker", **attribution)
                self.assertStop(self.stop_of(D), "stopped", D, "chain_cancelled", None)

    def test_a_fault_is_unknown_and_keeps_the_root(self):
        self.stop(A, "fault")
        self.assertStop(self.stop_of(A), "unknown", A)
        self.assertStop(self.stop_of(C), "unknown", A)

    def test_a_chain_member_observes_the_marker_of_its_stored_root(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        for member_id in (B, C):
            self.assertStop(self.stop_of(member_id), "stopped", A, "chain_cancelled", B)
        # A standalone task is not part of the chain.
        self.assertStop(self.stop_of(D), "not_stopped", D)

    def test_a_marker_keyed_by_a_member_is_not_its_chains_marker(self):
        self.stop(B, "marker", kind="valid", cancelled_task_id=C)
        self.assertStop(self.stop_of(B), "not_stopped", A)
        self.assertStop(self.stop_of(A), "not_stopped", A)

    def test_an_invalid_stored_root_is_unknown_with_a_null_root(self):
        self.assertStop(self.stop_of(E), "unknown", None)
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        self.assertStop(self.stop_of(E), "unknown", None)

    def test_an_invalid_root_matching_a_stop_row_stays_unknown_with_a_null_root(self):
        self.sql("INSERT INTO chain_stops VALUES(?,?,NULL)", (BAD_ROOT, "marker"))
        self.assertStop(self.stop_of(E), "unknown", None)

    def test_a_marker_whose_attribution_is_not_utf8_is_stopped_with_a_null_cancelled_task(self):
        self.sql("INSERT INTO chain_stops VALUES(?,?,CAST(x'80' AS TEXT))", (A, "marker"))
        self.assertStop(self.stop_of(A), "stopped", A, "chain_cancelled", None)
        self.assertStop(self.stop_of(B, "--brief"), "stopped", A, "chain_cancelled", None)

    def test_an_invalid_root_matching_a_non_utf8_row_stays_unknown_without_failing(self):
        self.sql("INSERT INTO chain_stops VALUES(?,?,CAST(x'80' AS TEXT))", (BAD_ROOT, "marker"))
        self.assertStop(self.stop_of(E), "unknown", None)

    def test_an_empty_stored_root_is_unknown_with_a_null_root(self):
        self.add(F, started_at=day(15), finished_at=day(15), continuation_of=A, chain_root="")
        self.assertStop(self.stop_of(F), "unknown", None)

    def test_a_prefix_reads_the_same_object(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        self.assertStop(self.stop_of(C[:8]), "stopped", A, "chain_cancelled", B)

    def test_the_object_is_present_in_plain_text_output_too(self):
        done = self.read("status", A)
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        self.assertIn("continuation_stop", done.stdout)


class BriefStopTests(StopCase):
    def test_the_brief_object_equals_the_status_object_for_every_state(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        self.stop(D, "fault")
        for task_id in (A, B, C, D, E):
            with self.subTest(task=task_id):
                full = self.stop_of(task_id)
                brief = self.stop_of(task_id, "--brief")
                for name in ("state", "chain_root_task_id", "reason", "cancelled_task_id"):
                    self.assertEqual(brief[name], full[name], name)
                self.check_stamp(brief["observed_at"])

    def test_the_brief_of_a_stopped_chain_reports_it(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        self.assertStop(self.stop_of(C, "--brief"), "stopped", A, "chain_cancelled", B)

    def test_the_brief_of_an_unstopped_chain(self):
        self.assertStop(self.stop_of(C, "--brief"), "not_stopped", A)


class BatchStopTests(StopCase):
    def batch(self, *ids):
        done = self.read("status-batch", "--task-ids", ",".join(ids), "--json")
        self.assertEqual((done.returncode, done.stderr), (0, ""), done.stdout)
        document = json.loads(done.stdout, object_pairs_hook=OrderedDict)
        self.assertFalse(document["errors"])
        return document["tasks"]

    def test_each_row_carries_its_own_object_in_request_order(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=B)
        rows = self.batch(D, C, E, A)
        self.assertEqual([row["task_id"] for row in rows], [D, C, E, A])
        for row in rows:
            self.assertEqual(list(row["continuation_stop"]), KEYS)
        self.assertStop(rows[0]["continuation_stop"], "not_stopped", D)
        self.assertStop(rows[1]["continuation_stop"], "stopped", A, "chain_cancelled", B)
        self.assertStop(rows[2]["continuation_stop"], "unknown", None)
        self.assertStop(rows[3]["continuation_stop"], "stopped", A, "chain_cancelled", B)

    def test_non_utf8_attributions_do_not_fail_the_batch(self):
        self.sql("INSERT INTO chain_stops VALUES(?,?,CAST(x'80' AS TEXT))", (A, "marker"))
        self.sql("INSERT INTO chain_stops VALUES(?,?,CAST(x'80' AS TEXT))", (BAD_ROOT, "marker"))
        rows = self.batch(A, C, E, D)
        self.assertStop(rows[0]["continuation_stop"], "stopped", A, "chain_cancelled", None)
        self.assertStop(rows[1]["continuation_stop"], "stopped", A, "chain_cancelled", None)
        self.assertStop(rows[2]["continuation_stop"], "unknown", None)
        self.assertStop(rows[3]["continuation_stop"], "not_stopped", D)

    def test_a_row_equals_the_single_status_object(self):
        self.stop(A, "fault")
        for task_id, row in zip((A, B, D, E), self.batch(A, B, D, E)):
            single = self.stop_of(task_id)
            for name in ("state", "chain_root_task_id", "reason", "cancelled_task_id"):
                self.assertEqual(row["continuation_stop"][name], single[name], (task_id, name))

    def test_a_prefix_row_carries_the_object_of_the_resolved_task(self):
        self.stop(A, "marker", kind="valid", cancelled_task_id=C)
        (row,) = self.batch(B[:8])
        self.assertEqual(row["resolved_from"], {"input": B[:8], "task_id": B})
        self.assertStop(row["continuation_stop"], "stopped", A, "chain_cancelled", C)

    def test_list_rows_do_not_carry_the_object(self):
        done = self.good("list")
        for task in done["tasks"]:
            self.assertNotIn("continuation_stop", task)


if __name__ == "__main__":
    unittest.main()

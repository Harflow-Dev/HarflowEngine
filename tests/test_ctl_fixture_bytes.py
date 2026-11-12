"""Byte comparison of harflow-ctl against every fixture added to the bundle since 0.0.11.

Each fixture that tests/fixtures/contracts-v1.provenance.json classifies as `added` is either
reproduced with the real binary over a root seeded with hf-fixture-seed and compared under the
field policy of bytes_support (plan R3-D: deterministic inputs, the named SUBSTITUTIONS, exact bytes
everywhere else), or listed in EXCLUDED with its reason. docs/fixture-byte-comparison.md lists the
same substitutions and exclusions; test_the_document_lists_every_substitution_and_exclusion keeps
them in step.
"""

import unittest
from pathlib import Path

import bytes_artifacts as artifacts
import bytes_pending as pending
import bytes_stop as stop
import bytes_support as policy
from reads_support import ReadsCase

DOC = Path(__file__).resolve().parents[1] / "docs" / "fixture-byte-comparison.md"
VERSION_INFO = "negative version-info document that no command produces"
CANCEL = "needs task-cancel-cascade.v1 (E2 write)"
CANCEL_CASES = (
    "incomplete", "partial", "pending-child-withdrawal-unsettled", "pending", "refused-documents", "settled",
    "truncated", "unknown", "unrecorded",
)
EXCLUDED = {
    f"invalid-s05-missing-task-{name}.json": VERSION_INFO
    for name in ("artifacts", "cancel-cascade", "continuation-stop", "continue", "pending-launch")
}
EXCLUDED.update({f"ctl-task-cancel-{name}.json": CANCEL for name in CANCEL_CASES})
EXCLUDED.update({f"ctl-task-pending-{name}.json": why for name, why in pending.EXCLUDED.items()})
EXCLUDED.update({f"ctl-task-artifacts-{name}.json": why for name, why in artifacts.EXCLUDED.items()})
EXCLUDED.update(stop.EXCLUDED)
COMPARED = (
    [f"ctl-task-pending-{name}.json" for name in pending.SCENARIOS]
    + [f"ctl-task-artifacts-{name}.json" for name in artifacts.SCENARIOS]
    + [f"{name}.json" for name in stop.NAMES]
)


class CoverageTests(unittest.TestCase):
    def test_every_added_fixture_is_compared_or_excluded_exactly_once(self):
        added = policy.provenance_added()
        self.assertEqual(len(added), 43)
        self.assertEqual(len(COMPARED), len(set(COMPARED)))
        self.assertEqual(set(COMPARED) & set(EXCLUDED), set())
        self.assertEqual(sorted(COMPARED + list(EXCLUDED)), added)

    def test_the_document_lists_every_substitution_and_exclusion(self):
        text = DOC.read_text(encoding="utf-8")
        for name in policy.SUBSTITUTIONS:
            self.assertIn(f"`{name}`", text, name)
        for name in EXCLUDED:
            self.assertIn(f"`{name}`", text, name)
        keys = set(policy.RUST_ONLY) | {"session_id"}
        for kind in ("status", "brief"):
            self.assertEqual(policy.SEED_ABSENT[kind] & policy.RUST_UNPROJECTED[kind], set())
            keys |= policy.SEED_ABSENT[kind] | policy.RUST_UNPROJECTED[kind]
        for key in sorted(keys):
            self.assertIn(f"`{key}`", text, key)


class FixtureBytesTests(ReadsCase):
    def test_pending(self):
        for scenario in pending.SCENARIOS:
            name = f"ctl-task-pending-{scenario}.json"
            with self.subTest(fixture=name):
                done, rules = pending.run(self, scenario)
                policy.compare(self, name, done, rules)

    def test_artifacts(self):
        for scenario in artifacts.SCENARIOS:
            name = f"ctl-task-artifacts-{scenario}.json"
            with self.subTest(fixture=name):
                done, rules = artifacts.run(self, scenario)
                policy.compare(self, name, done, rules)

    def test_continuation_stop(self):
        for name in stop.NAMES:
            with self.subTest(fixture=name):
                done, rules = stop.run(self, name)
                policy.compare(self, name + ".json", done, rules)


if __name__ == "__main__":
    unittest.main()

"""Verify the published canonical contract fixture bundle and audit trail."""

import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "tests" / "fixtures" / "contracts-v1"
EXPECTED_MANIFEST_SHA256 = "56ac4ce3c762ebb3b40855fc6defa47636076cb9b2ebdda9d7b207dce350fb55"
EXPECTED_OLD_PROVENANCE_SHA256 = "49e2708616aecb29cba324a797de06314db77ac9ea067f479ffe991267a04311"

ADDED_FAMILIES = {
    "task-continue",
    "task-artifacts",
    "task-cancel-cascade",
    "task-pending-launch",
    "task-continuation-stop",
}


def _sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _fixture(name):
    return json.loads((BUNDLE / name).read_bytes())


class ContractFixtureBundleTests(unittest.TestCase):
    def test_exact_pinned_manifest_and_all_fixture_hashes(self):
        manifest_bytes = (BUNDLE / "manifest.json").read_bytes()
        self.assertEqual(hashlib.sha256(manifest_bytes).hexdigest(), EXPECTED_MANIFEST_SHA256)
        self.assertEqual(
            (BUNDLE / "manifest.sha256").read_text(encoding="ascii"),
            f"{EXPECTED_MANIFEST_SHA256}  manifest.json\n",
        )

        manifest = json.loads(manifest_bytes)
        cases = manifest["cases"]
        hashes = manifest["files"]
        self.assertEqual(len(cases), 334)
        self.assertEqual(set(cases), set(hashes))
        actual_names = {
            path.name
            for path in BUNDLE.iterdir()
            if path.name not in {"manifest.json", "manifest.sha256"}
        }
        self.assertEqual(actual_names, set(hashes))
        for name, expected in hashes.items():
            self.assertEqual(_sha256(BUNDLE / name), expected, name)

    def test_v3_provenance_pins_public_artifacts_and_preserves_the_previous_history(self):
        path = ROOT / "tests" / "fixtures" / "contracts-v1.provenance.json"
        provenance = json.loads(path.read_bytes())
        manifest = json.loads((BUNDLE / "manifest.json").read_bytes())
        self.assertEqual(provenance["consumer_audit_version"], 3)
        self.assertEqual(provenance["public_version"], "0.0.13")
        self.assertEqual(provenance["source_commit"], "0ac7e3cc07a2adc8602e6d378549c99184247f93")
        self.assertEqual(provenance["tag"], "v0.0.13")
        self.assertEqual(provenance["release_url"], "https://github.com/Harflow-Dev/HarflowMCP/releases/tag/v0.0.13")
        self.assertIs(provenance["release_prerelease"], True)
        self.assertIn("Local tag v0.0.13 resolves to source commit", provenance["source_identity_basis"])
        self.assertEqual(provenance["contracts_md_sha256"], "d8d1d715f9af87a16720253e0ebe94b19ec2c78be9853ec942615304a5f32a3f")
        self.assertEqual(provenance["wheel_sha256"], "1363c549e8d701efb9a0d6f00197f725a262f5fdecca6ff2964f5331d60c8424")
        self.assertEqual(provenance["sdist_sha256"], "64303a47c3583b8d9622323e639fdad6ef5dc36e4d0ee508e5af4c2e8971dff7")
        self.assertEqual(provenance["manifest_sha256"], EXPECTED_MANIFEST_SHA256)
        self.assertEqual(provenance["fixture_count"], 334)
        self.assertEqual(provenance["previous_fixture_count"], 291)
        self.assertEqual(provenance["previous_manifest_sha256"], "cd58a5ef8bae565513669dcf4d4ac9179ea327d5093de4461806d7be8dae1873")
        self.assertEqual(provenance["removed"], [])

        history = provenance["history"]
        self.assertEqual(history["path"], "tests/fixtures/provenance/contracts-v1-0.0.11.json")
        old_provenance = ROOT / history["path"]
        self.assertEqual(history["sha256"], EXPECTED_OLD_PROVENANCE_SHA256)
        self.assertEqual(_sha256(old_provenance), EXPECTED_OLD_PROVENANCE_SHA256)

    def test_every_audit_entry_has_hashes_and_classified_json_pointer_diffs(self):
        provenance = json.loads((ROOT / "tests" / "fixtures" / "contracts-v1.provenance.json").read_bytes())
        manifest = json.loads((BUNDLE / "manifest.json").read_bytes())
        old_provenance = json.loads((ROOT / "tests" / "fixtures" / "provenance" / "contracts-v1-0.0.11.json").read_bytes())
        self.assertEqual(len(provenance["files"]), 334)
        entries = {entry["file"]: entry for entry in provenance["files"]}
        self.assertEqual(set(entries), set(manifest["files"]))
        self.assertEqual(len(entries), 334)
        old_entries = {entry["file"]: entry for entry in old_provenance["files"]}
        self.assertEqual(len(old_provenance["files"]), 291)
        self.assertEqual(set(old_entries), {name for name, entry in entries.items() if entry["classification"] != "added"})
        expected_counts = {
            "product-version-only": 190,
            "capability-list": 29,
            "opaque-regeneration": 15,
            "usage-help": 1,
            "unchanged": 56,
            "added": 43,
        }
        observed_counts = {}
        for entry in entries.values():
            name = entry["file"]
            self.assertEqual(entry["sha256"], manifest["files"][name], name)
            self.assertEqual(_sha256(BUNDLE / name), entry["sha256"], name)
            self.assertIn("previous_sha256", entry, name)
            if entry["classification"] == "added":
                self.assertIsNone(entry["previous_sha256"], name)
            else:
                self.assertEqual(entry["previous_sha256"], old_entries[name]["sha256"], name)
                if entry["classification"] == "unchanged":
                    self.assertEqual(entry["sha256"], entry["previous_sha256"], name)
                else:
                    self.assertNotEqual(entry["sha256"], entry["previous_sha256"], name)
            if entry["previous_sha256"] is not None:
                self.assertRegex(entry["previous_sha256"], r"^[0-9a-f]{64}$", name)
            self.assertIsInstance(entry["changes"], list, name)
            for change in entry["changes"]:
                self.assertEqual(len(change), 3, name)
                self.assertIsInstance(change[0], str, name)
                self.assertTrue(change[0].startswith("/"), name)
            classification = entry["classification"]
            observed_counts[classification] = observed_counts.get(classification, 0) + 1
            paths = {change[0] for change in entry["changes"]}
            if classification == "product-version-only":
                self.assertEqual(entry["previous_sha256"] is None, False, name)
                self.assertTrue(paths)
                self.assertTrue(all(item == "/product_version" or item.endswith("/product_version") for item in paths), name)
            elif classification == "capability-list":
                self.assertTrue(entry["previous_sha256"])
                self.assertIn("/capabilities", paths, name)
                self.assertTrue(paths <= {"/capabilities", "/product_version"}, name)
            elif classification == "opaque-regeneration":
                allowed = {"/product_version", "/result/change_token", "/result/generation", "/result/newer_cursor", "/result/next_cursor", "/result/older_cursor", "/result/snapshot_id"}
                self.assertTrue(entry["previous_sha256"])
                self.assertTrue(paths)
                self.assertTrue(all(item in allowed or (item.startswith("/result/events/") and item.endswith("/product_version")) for item in paths), name)
            elif classification == "unchanged":
                self.assertTrue(entry["previous_sha256"])
                self.assertEqual(paths, set(), name)
            elif classification == "usage-help":
                self.assertTrue(entry["previous_sha256"])
                self.assertEqual(paths, {"/product_version", "/error/message"}, name)
            elif classification == "added":
                self.assertIsNone(entry["previous_sha256"], name)
                self.assertEqual(paths, set(), name)
                self.assertIn(entry["added_family"], ADDED_FAMILIES, name)
            else:
                self.fail(f"unrecognized audit classification {classification!r}: {name}")

        self.assertEqual(provenance["counts"], expected_counts)
        self.assertEqual(observed_counts, expected_counts)
        family_counts = {}
        for entry in entries.values():
            family = entry.get("added_family")
            if family is not None:
                family_counts[family] = family_counts.get(family, 0) + 1
        self.assertEqual(family_counts, {
            "task-continue": 5,
            "task-artifacts": 8,
            "task-cancel-cascade": 10,
            "task-pending-launch": 14,
            "task-continuation-stop": 6,
        })

    def test_new_fixture_families_preserve_advertisement_and_unknown_boundaries(self):
        version_info = _fixture("version-info.json")
        advertised = json.dumps(version_info["capabilities"], sort_keys=True)
        self.assertNotIn("workflow-steering-poststart.v1", advertised)

        claimed = _fixture("ctl-conversation-now-claimed.json")["result"]
        self.assertEqual(claimed["messages"][-1]["status"], "claimed")
        self.assertEqual(claimed["cumulative_issuances"], 1)
        unknown = _fixture("ctl-conversation-delivery-unknown.json")["result"]
        self.assertEqual(unknown["messages"][-1]["status"], "unknown")
        self.assertEqual(unknown["progress"]["phase"], "unknown")
        self.assertEqual(unknown["cumulative_issuances"], 1)
        queued = _fixture("ctl-conversation-now-queued.json")["result"]
        self.assertEqual(queued["messages"][-1]["status"], "queued")
        self.assertEqual(queued["cumulative_issuances"], 0)

    def test_steering_fixture_budgets_keep_finite_and_unlimited_specimens_distinct(self):
        finite_window = _fixture("ctl-steering-orchestrator-nested-ready.json")["result"]
        self.assertEqual(finite_window["protocol_upgrade"]["schema_version"], 2)
        self.assertEqual(finite_window["budgets"]["root_limit"], 32)
        self.assertEqual(finite_window["budgets"]["episode_limit"], 8)

        for name in ("ctl-steering-worker-ready.json", "ctl-steering-window-upgraded-status.json"):
            unlimited = _fixture(name)["result"]["budgets"]
            self.assertIsNone(unlimited["root_limit"], name)
            self.assertIsNone(unlimited["episode_limit"], name)
            self.assertEqual(unlimited["harness_turn_cap_maximum"], 8, name)


if __name__ == "__main__":
    unittest.main()

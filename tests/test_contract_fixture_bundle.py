"""Verify the vendored canonical contract fixture bundle's pinned provenance."""

import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "tests" / "fixtures" / "contracts-v1"
EXPECTED_MANIFEST_SHA256 = "ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906"


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
        self.assertEqual(len(cases), 207)
        self.assertEqual(set(cases), set(hashes))
        actual_names = {
            path.name
            for path in BUNDLE.iterdir()
            if path.name not in {"manifest.json", "manifest.sha256"}
        }
        self.assertEqual(actual_names, set(hashes))
        for name, expected in hashes.items():
            self.assertEqual(hashlib.sha256((BUNDLE / name).read_bytes()).hexdigest(), expected, name)

    def test_full_bundle_provenance_matches_source_and_classified_delta(self):
        provenance_path = ROOT / "tests" / "fixtures" / "contracts-v1.provenance.json"
        provenance = json.loads(provenance_path.read_bytes())
        manifest = json.loads((BUNDLE / "manifest.json").read_bytes())
        self.assertEqual(provenance["tag"], "v0.0.10")
        self.assertEqual(provenance["source_commit"], "8a936de61b5f9f2fdff671b65b5384259267350b")
        self.assertEqual(provenance["contracts_md_sha256"], "780170354755824d324ed9804f132f0fae6b070c5315bf09393642db2d89264d")
        self.assertEqual(provenance["wheel_sha256"], "c6ada9fd00c1ff85faf9fd94165b84820777bfcda5be5641bf00d300163ac8b9")
        self.assertEqual(provenance["sdist_sha256"], "5b887fa4e26d3a61dd6db416e946149b7d0561fe4d8e23c4578fa102ccdb35e9")
        self.assertEqual(provenance["manifest_sha256"], EXPECTED_MANIFEST_SHA256)
        self.assertEqual(provenance["fixture_count"], 207)
        self.assertEqual(provenance["previous_fixture_count"], 199)
        self.assertEqual(provenance["previous_manifest_sha256"], "383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0")
        self.assertEqual(provenance["removed"], [])

        entries = {entry["file"]: entry for entry in provenance["files"]}
        self.assertEqual(set(entries), set(manifest["files"]))
        self.assertEqual(len(entries), 207)
        for name, expected in manifest["files"].items():
            entry = entries[name]
            self.assertEqual(entry["sha256"], expected, name)
            self.assertEqual(hashlib.sha256((BUNDLE / name).read_bytes()).hexdigest(), entry["sha256"], name)

        expected_counts = {
            "product-version-only": 104,
            "unchanged": 69,
            "opaque-regeneration": 17,
            "capability-list": 8,
            "usage-help": 1,
            "watch-contract-added": 7,
            "watch-capability-negative": 1,
        }
        observed_counts = {}
        for entry in entries.values():
            observed_counts[entry["classification"]] = observed_counts.get(entry["classification"], 0) + 1
        self.assertEqual(provenance["counts"], expected_counts)
        self.assertEqual(observed_counts, expected_counts)

        for entry in entries.values():
            paths = {change[0] for change in entry["changes"]}
            classification = entry["classification"]
            if classification == "product-version-only":
                self.assertTrue(paths)
                self.assertTrue(all(path == "/product_version" or path.endswith("/product_version") for path in paths), entry["file"])
            elif classification == "capability-list":
                self.assertTrue(paths <= {"/capabilities", "/product_version"}, entry["file"])
                self.assertIn("/capabilities", paths, entry["file"])
            elif classification == "opaque-regeneration":
                allowed = {"/product_version", "/result/change_token", "/result/generation", "/result/newer_cursor", "/result/next_cursor", "/result/older_cursor", "/result/snapshot_id"}
                self.assertTrue(paths, entry["file"])
                self.assertTrue(all(path in allowed or (path.startswith("/result/events/") and path.endswith("/product_version")) for path in paths), entry["file"])
            elif classification == "unchanged":
                self.assertEqual(paths, set(), entry["file"])
            elif classification in {"watch-contract-added", "watch-capability-negative"}:
                self.assertEqual(paths, set(), entry["file"])
                self.assertIsNone(entry["previous_sha256"], entry["file"])
            else:
                self.assertEqual(paths, {"/product_version", "/error/message"}, entry["file"])


if __name__ == "__main__":
    unittest.main()

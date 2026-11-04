"""Verify the vendored canonical contract fixture bundle's pinned provenance."""

import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "tests" / "fixtures" / "contracts-v1"
EXPECTED_MANIFEST_SHA256 = "383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0"


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
        self.assertEqual(len(cases), 199)
        self.assertEqual(set(cases), set(hashes))
        actual_names = {
            path.name
            for path in BUNDLE.iterdir()
            if path.name not in {"manifest.json", "manifest.sha256"}
        }
        self.assertEqual(actual_names, set(hashes))
        for name, expected in hashes.items():
            self.assertEqual(hashlib.sha256((BUNDLE / name).read_bytes()).hexdigest(), expected, name)


if __name__ == "__main__":
    unittest.main()

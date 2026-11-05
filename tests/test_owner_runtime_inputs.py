"""Integrity checks for the pinned, synthetic owner-runtime input bundle."""

import hashlib
import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests" / "fixtures"
COMPONENT = FIXTURES / "owner-runtime-v1"
SOURCE = FIXTURES / "owner-runtime-v1-source.json"
PINNED_COMMIT = "4f5b86d27d249094d7378c84029a94e2aaec966d"
PINNED_MANIFEST = "7df0c93a17b185620e35fcf0d0a3e060174ff0445318ed505dc7e4573a08c6d6"
PINNED_SPECS = {
    "docs/decisions/owner-runtime-v1.md":
        "448c74da6acf5955de5114bdfee2ec149e20e43656c287f44b3c6300c3a53897",
    "docs/decisions/owner-runtime-v1-schema.md":
        "92ecc534085ccc27550a1377bbce4273906366792a3f4c313384bf2d808847de",
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class OwnerRuntimeInputIntegrityTests(unittest.TestCase):
    def test_source_pin_and_specification_hashes_are_exact(self):
        source = json.loads(SOURCE.read_text(encoding="utf-8"))
        self.assertEqual(source["commit"], PINNED_COMMIT)
        self.assertEqual(source["manifest_sha256"], PINNED_MANIFEST)
        self.assertEqual(source["status"], "specified_unimplemented")
        self.assertEqual(source["provenance"], "synthetic_specification")
        self.assertIs(source["python_runtime_support"], False)
        self.assertEqual(
            {item["path"]: item["sha256"] for item in source["specifications"]},
            PINNED_SPECS,
        )
        self.assertEqual(source["supported_baseline_unchanged"]["version"], "0.0.10")
        self.assertEqual(source["supported_baseline_unchanged"]["cases"], 207)

    def test_component_manifest_digest_and_inventory_are_exact(self):
        manifest_bytes = (COMPONENT / "manifest.json").read_bytes()
        digest_bytes = (COMPONENT / "manifest.sha256").read_bytes()
        expected_digest = sha256(manifest_bytes)
        self.assertEqual(expected_digest, PINNED_MANIFEST)
        self.assertEqual(digest_bytes, (expected_digest + "\n").encode("ascii"))

        manifest = json.loads(manifest_bytes.decode("utf-8"))
        self.assertEqual(manifest["format"], "harflow.owner-runtime.profile.component")
        self.assertEqual(manifest["profile"], "owner-runtime-v1")
        self.assertEqual(manifest["status"], "specified_unimplemented")
        self.assertEqual(manifest["provenance"], "synthetic_specification")
        self.assertIs(manifest["python_runtime_support"], False)
        self.assertEqual(
            {item["path"]: item["sha256"] for item in manifest["specifications"]},
            PINNED_SPECS,
        )

        entries = manifest["files"]
        names = [entry["filename"] for entry in entries]
        self.assertEqual(len(names), 64)
        self.assertEqual(len(set(names)), len(names))
        self.assertTrue(all(re.fullmatch(r"[a-z0-9_-]+\.json", name) for name in names))
        actual_names = {
            path.name for path in COMPONENT.iterdir()
            if path.name.endswith(".json") and path.name != "manifest.json"
        }
        self.assertEqual(actual_names, set(names))

        for entry in entries:
            path = COMPONENT / entry["filename"]
            self.assertFalse(path.is_symlink(), entry["filename"])
            stat = path.stat()
            self.assertEqual(stat.st_nlink, 1, entry["filename"])
            content = path.read_bytes()
            self.assertEqual(len(content), entry["bytes"], entry["filename"])
            self.assertEqual(sha256(content), entry["sha256"], entry["filename"])
            self.assertIsInstance(entry["expected"], dict)

    def test_no_unlisted_component_entries(self):
        self.assertEqual(
            {path.name for path in COMPONENT.iterdir()},
            {"manifest.json", "manifest.sha256"}
            | {entry["filename"] for entry in json.loads(
                (COMPONENT / "manifest.json").read_text(encoding="utf-8")
            )["files"]},
        )


if __name__ == "__main__":
    unittest.main()

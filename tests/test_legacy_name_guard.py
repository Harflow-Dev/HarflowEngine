"""Behavioral tests for the legacy-name source guard."""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


GUARD = Path(__file__).resolve().parents[1] / "scripts" / "check-legacy-names.py"
PREDECESSOR = "poly" + "bridge"


class LegacyNameGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "project"
        scripts = self.root / "scripts"
        scripts.mkdir(parents=True)
        shutil.copy2(GUARD, scripts / GUARD.name)
        (scripts / "legacy-name-allowlist.json").write_text("[]", encoding="utf-8")

    def tearDown(self):
        self.temp.cleanup()

    def write(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content if isinstance(content, bytes) else content.encode())
        return path

    def run_guard(self):
        return subprocess.run(
            [sys.executable, str(self.root / "scripts" / GUARD.name)],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_rejects_forbidden_literal_in_source(self):
        self.write("src/module.rs", f"// {PREDECESSOR}\n")

        result = self.run_guard()

        self.assertEqual(result.returncode, 1)
        self.assertIn("Legacy name:", result.stderr)

    def test_accepts_exact_allowlisted_source_path(self):
        path = self.write("src/compat.rs", f"// {PREDECESSOR}\n")
        (self.root / "scripts" / "legacy-name-allowlist.json").write_text(
            json.dumps([path.relative_to(self.root).as_posix()]), encoding="utf-8"
        )

        result = self.run_guard()

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_excludes_nested_claude_worktrees(self):
        self.write(".claude/worktrees/example/src/note.txt", PREDECESSOR)
        self.write(".claude/notes.txt", "clean")

        result = self.run_guard()

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_scans_claude_sources_outside_worktrees(self):
        self.write(".claude/notes.txt", PREDECESSOR)

        result = self.run_guard()

        self.assertEqual(result.returncode, 1)

    def test_scans_other_worktrees_named_directories(self):
        self.write("other/worktrees/example/note.txt", PREDECESSOR)

        result = self.run_guard()

        self.assertEqual(result.returncode, 1)

    def test_retains_pruning_for_excluded_directory_parts_and_names(self):
        for directory in (".git", "target", "__pycache__"):
            self.write(f"src/{directory}/note.txt", PREDECESSOR)
        self.write("target", PREDECESSOR)

        result = self.run_guard()

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_scans_file_symlink_target_when_supported(self):
        target = Path(self.temp.name) / "external.txt"
        target.write_text(PREDECESSOR, encoding="utf-8")
        link = self.root / "src" / "linked.txt"
        link.parent.mkdir(parents=True, exist_ok=True)
        try:
            link.symlink_to(target)
        except OSError as error:
            self.skipTest(f"file symlinks unavailable: {error}")

        result = self.run_guard()

        self.assertEqual(result.returncode, 1)
        self.assertIn("src/linked.txt", result.stderr)


if __name__ == "__main__":
    unittest.main()

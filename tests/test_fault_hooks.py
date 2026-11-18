import importlib.util
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("check_fault_hooks", ROOT / "scripts/check-fault-hooks.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
HOME = Path("/virtual/src")


def run(files, roots):
    tree = {HOME / name: text for name, text in files.items()}
    return guard.source_check(lambda p: tree.get(p), [HOME / r for r in roots])


class FaultHookGuard(unittest.TestCase):
    def test_fault_hook_sources_are_test_only(self):
        self.assertEqual(guard.source_check(), [], "repository sources")
        seen = guard.reachable(guard.ROOTS, lambda p: p.read_text() if p.exists() else None)
        names = {p.as_posix() for p in seen}
        self.assertTrue(any(n.endswith("task_runtime/writer_runtime.rs") for n in names))
        self.assertFalse(any("task_runtime/seams.rs" in n or "task_runtime/tests/" in n for n in names))
        clean = {"lib.rs": "#[cfg(test)]\nmod seams;\npub fn f() {\n    #[cfg(test)]\n    super::seams::hit();\n}\n"}
        self.assertEqual(run(clean, ["lib.rs"]), [])
        self.assertTrue(run({"lib.rs": "pub fn f() {\n    super::seams::hit();\n}\n"}, ["lib.rs"]))
        self.assertTrue(run({"lib.rs": "#[cfg(not(test))]\nfn f() {}\n"}, ["lib.rs"]))
        self.assertTrue(run({"lib.rs": "#[cfg(test)]\nuse crate::seams as s;\n"}, ["lib.rs"]))
        self.assertTrue(run({"lib.rs": "fn f() { let _ = ThreadWriter; }\n"}, ["lib.rs"]))

    def test_reachability_is_modelled_or_rejected(self):
        planted = {
            "lib.rs": "mod task_runtime;\n",
            "task_runtime/mod.rs": "mod seams;\n",
            "task_runtime/seams.rs": "pub fn hit() {}\n",
        }
        problems = run(planted, ["lib.rs"])
        self.assertTrue(any("test-only file is reachable" in p for p in problems), problems)
        same_line = {"lib.rs": "#[cfg(test)] mod seams;\n"}
        self.assertEqual(run(same_line, ["lib.rs"]), [])
        same_line_live = {
            "lib.rs": "mod task_runtime;\n",
            "task_runtime/mod.rs": "#[allow(dead_code)] mod seams;\n",
            "task_runtime/seams.rs": "",
        }
        self.assertTrue(any("test-only" in p for p in run(same_line_live, ["lib.rs"])))
        self.assertTrue(any("unrecognized" in p for p in run({"lib.rs": "fn f() {} mod seams;\n"}, ["lib.rs"])))
        hidden = {"lib.rs": "#[cfg(test)] mod ignored; mod live;\n", "live.rs": ""}
        problems = run(hidden, ["lib.rs"])
        self.assertTrue(any("code after a module declaration" in p for p in problems), problems)
        hidden_seam = {"lib.rs": "#[cfg(test)] fn a() {} fn b() { seams::hit(); }\n"}
        self.assertTrue(run(hidden_seam, ["lib.rs"]))
        stacked = {"lib.rs": "#[cfg(test)]\n#[allow(dead_code)]\nmod seams;\n"}
        self.assertEqual(run(stacked, ["lib.rs"]), [])
        self.assertTrue(any("missing" in p for p in run({}, ["lib.rs"])))
        self.assertTrue(any("does not resolve" in p for p in run({"lib.rs": "mod gone;\n"}, ["lib.rs"])))
        self.assertTrue(any("inline module" in p for p in run({"lib.rs": "mod a {\n    mod seams;\n}\n"}, ["lib.rs"])))
        self.assertEqual(run({"lib.rs": '#[path = "other/x.rs"]\nmod x;\n', "other/x.rs": ""}, ["lib.rs"]), [])

    def test_planted_symbols_are_detected(self):
        self.assertTrue(guard.scan_symbols("0000 T __ZN8hf_host12task_runtime5seams9dispatch17h0E"))
        self.assertTrue(guard.scan_symbols("0000 T _RNvNtCs1_7hf_host5seams4hit"))
        self.assertFalse(guard.scan_symbols("0000 T __ZN8hf_host12task_runtime10run_writer17h0E"))

    def test_release_build_contains_no_fault_hooks(self):
        paths = [os.environ.get(k) for k in ("HF_RELEASE_CTL", "HF_RELEASE_SCAFFOLD", "HF_SEAM_CONTROL_BIN")]
        if not all(paths):
            if os.environ.get("HF_S2_LANE"):
                self.fail("the S2 lane requires HF_RELEASE_CTL, HF_RELEASE_SCAFFOLD and HF_SEAM_CONTROL_BIN")
            self.skipTest("HF_RELEASE_CTL, HF_RELEASE_SCAFFOLD and HF_SEAM_CONTROL_BIN are not set")
        missing = [p for p in paths if not Path(p).exists()]
        self.assertEqual(missing, [], "release artifacts are missing")
        self.assertEqual(guard.binary_check(*map(Path, paths)), [])

    def test_control_fixture_builds_and_is_flagged(self):
        if shutil.which("rustc") is None:
            self.skipTest("rustc is not on PATH")
        with tempfile.TemporaryDirectory() as out:
            binary = Path(out) / "control"
            built = subprocess.run(
                ["rustc", "-C", "opt-level=3", str(ROOT / "tools/fault-hook-control/control.rs"), "-o", str(binary)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            self.assertTrue(guard.scan_symbols(guard.nm(binary)))


if __name__ == "__main__":
    unittest.main()

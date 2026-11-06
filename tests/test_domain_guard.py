import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class DomainGuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.environment = os.environ.copy()
        cargo_home = cls.environment.get("CARGO_HOME")
        if cargo_home:
            cls.environment["PATH"] = os.pathsep.join(
                [str(Path(cargo_home) / "bin"), cls.environment.get("PATH", "")]
            )
        cargo = shutil.which("cargo", path=cls.environment["PATH"])
        if cargo is None:
            raise RuntimeError("cargo is required for the domain guard tests")
        subprocess.run([cargo, "build", "--locked", "-p", "hf-architecture"], cwd=ROOT, env=cls.environment, check=True)
        metadata = subprocess.check_output([cargo, "metadata", "--locked", "--format-version", "1"], cwd=ROOT, env=cls.environment, encoding="utf-8")
        target = Path(json.loads(metadata)["target_directory"])
        cls.binary = target / "debug" / ("hf-architecture.exe" if os.name == "nt" else "hf-architecture")

    def scan(self, sources):
        with tempfile.TemporaryDirectory(prefix="harflow-domain-guard-") as temporary:
            root = Path(temporary)
            for filename, source in sources.items():
                path = root / filename
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            return subprocess.run([self.binary, root], capture_output=True, encoding="utf-8")

    def test_accepts_pure_value_types_and_literal_documentation(self):
        result = self.scan({"src/lib.rs": '/// A domain value.\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct Value(Option<u32>);\n'})
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_allows_boxed_value_types_but_rejects_effectful_inner_types(self):
        allowed = self.scan({
            "src/lib.rs": (
                "use std::boxed::Box;\n"
                "#[derive(Clone, Debug, Eq, PartialEq)]\n"
                "pub struct Value(Box<Option<Vec<String>>>);\n"
            )
        })
        self.assertEqual(allowed.returncode, 0, allowed.stderr)

        samples = {
            "direct effectful inner type":
                "pub struct Value(std::boxed::Box<std::fs::File>);",
            "aliased effectful inner type":
                "use std::boxed::Box as Heap; pub struct Value(Heap<std::process::Command>);",
            "grouped effectful inner type":
                "use std::{boxed::Box, fs::File}; pub struct Value(Box<File>);",
        }
        for name, source in samples.items():
            with self.subTest(name=name):
                result = self.scan({"src/lib.rs": source})
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_allows_inline_unit_test_modules_but_not_other_cfg_io(self):
        allowed = self.scan({
            "src/lib.rs": "#[cfg(test)] mod tests { #[test] fn value() { assert_eq!(1, 1); } }\n"
        })
        self.assertEqual(allowed.returncode, 0, allowed.stderr)

        rejected = self.scan({
            "src/lib.rs": '#[cfg(feature = "diagnostics")] mod diagnostics { pub fn read() { let _ = std::fs::read("x"); } }\n'
        })
        self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)

    def test_rejects_direct_and_aliased_filesystem_and_process_access(self):
        samples = {
            "direct filesystem": 'pub fn read() { let _ = std::fs::read("x"); }',
            "direct process": 'pub fn run() { let _ = std::process::Command::new("x"); }',
            "renamed module": 'use std as sys; pub fn read() { let _ = sys::fs::read("x"); }',
            "renamed import": 'use std::process as proc; pub fn run() { let _ = proc::id(); }',
            "grouped import": 'use std::{fs, process};',
            "OS access": 'use std::os::raw::c_char;',
        }
        for name, source in samples.items():
            with self.subTest(name=name):
                result = self.scan({"src/lib.rs": source})
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_rejects_aliases_globs_extern_crate_macros_and_unsafe(self):
        samples = [
            'use std as host;',
            'use std::*;',
            'extern crate std;',
            'pub fn value() { println!("value"); }',
            'pub fn value() { unsafe { core::hint::unreachable_unchecked() } }',
            '#[cfg(unix)] pub struct Value;',
        ]
        for source in samples:
            with self.subTest(source=source):
                result = self.scan({"src/lib.rs": source})
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_rejects_macros_hidden_in_doc_attributes(self):
        for expression in ['include_str!("secret.txt")', 'env!("HOME")', 'concat!("value")']:
            with self.subTest(expression=expression):
                result = self.scan({"src/lib.rs": f'#[doc = {expression}]\npub struct Value;\n'})
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_scans_nested_domain_source_roots(self):
        result = self.scan({
            "src/lib.rs": "pub struct Value;\n",
            "build.rs": 'fn main() { let _ = std::process::Command::new("x"); }',
            "targets/domain.rs": 'pub fn read() { let _ = std::fs::read("x"); }',
        })
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()

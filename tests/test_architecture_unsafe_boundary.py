import importlib.util
import unittest
from pathlib import Path

BASE_PATH = Path(__file__).resolve().parent / "test_architecture_metadata.py"
SPEC = importlib.util.spec_from_file_location("architecture_metadata_fixture", BASE_PATH)
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
validate = FIXTURE.validate

MACOS = 'cfg(target_os = "macos")'
LIBC = {
    "name": "libc", "kind": None, "req": "=0.2.191",
    "source": "registry+https://github.com/rust-lang/crates.io-index",
    "uses_default_features": False, "features": [],
    "optional": False, "target": MACOS, "rename": None,
}


class UnsafeBoundaryTests(unittest.TestCase):
    setUp = FIXTURE.ArchitectureMetadataTests.setUp
    tearDown = FIXTURE.ArchitectureMetadataTests.tearDown

    def package(self, name):
        return next(item for item in self.packages if item["name"] == name)

    def sys_edge(self, **change):
        edge = {
            "name": "hf-sys", "kind": None, "req": "*", "source": None,
            "path": str(self.root / "crates/hf-sys"),
            "uses_default_features": True, "features": [],
            "optional": False, "target": MACOS, "rename": None,
        }
        return {**edge, **change}

    def accepted_edges(self):
        self.package("hf-sys")["dependencies"] = [dict(LIBC)]
        self.package("hf-platform")["dependencies"] = [self.sys_edge()]

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_accepts_the_real_shaped_edges(self):
        self.accepted_edges()
        validate(self.metadata, self.root)

    def test_rejects_hf_sys_lint_drift(self):
        manifest = Path(self.package("hf-sys")["manifest_path"])
        accepted = manifest.read_text(encoding="utf-8")
        for label, text in (
            ("extra lint", accepted + 'unused = "warn"\n'),
            ("rust allow", accepted.replace('unsafe_code = "deny"', 'unsafe_code = "allow"')),
            ("rust forbid", accepted.replace('unsafe_code = "deny"', 'unsafe_code = "forbid"')),
            ("clippy drift", accepted.replace('panic = "deny"', 'panic = "warn"')),
            ("clippy missing", accepted.replace('todo = "deny"\n', "")),
            ("workspace inheritance", "[lints]\nworkspace = true\n"),
        ):
            manifest.write_text(text, encoding="utf-8")
            with self.subTest(label=label), self.assertRaisesRegex(ValueError, "hf-sys lint policy"):
                validate(self.metadata, self.root)

    def test_other_packages_must_inherit_the_workspace_lints(self):
        for name in ("hf-domain", "hf-platform", "hf-host"):
            manifest = Path(self.package(name)["manifest_path"])
            accepted = manifest.read_text(encoding="utf-8")
            manifest.write_text(accepted.replace("workspace = true", 'workspace = false'), encoding="utf-8")
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "Workspace lint inheritance"):
                validate(self.metadata, self.root)
            manifest.write_text(accepted, encoding="utf-8")

    def test_root_may_not_relax_unsafe_code(self):
        root = self.root / "Cargo.toml"
        root.write_text(root.read_text(encoding="utf-8").replace('"forbid"', '"deny"'), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "forbid unsafe_code"):
            validate(self.metadata, self.root)

    def test_libc_is_exact_registry_macos_and_owned_by_hf_sys(self):
        self.accepted_edges()
        self.package("hf-platform")["dependencies"].append(dict(LIBC))
        with self.assertRaisesRegex(ValueError, "Forbidden dependency"):
            validate(self.metadata, self.root)
        self.package("hf-platform")["dependencies"].pop()
        for change in (
            {"req": "0.2.191"}, {"req": "=0.2.190"}, {"uses_default_features": True},
            {"features": ["std"]}, {"optional": True}, {"target": None},
            {"target": "cfg(unix)"}, {"rename": "libc2"}, {"path": "/somewhere/libc"},
            {"source": None}, {"source": "git+https://example.invalid/libc"},
        ):
            self.package("hf-sys")["dependencies"] = [{**LIBC, **change}]
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "libc requires"):
                validate(self.metadata, self.root)

    def test_hf_sys_may_not_gain_other_dependencies(self):
        self.accepted_edges()
        self.package("hf-sys")["dependencies"].append({"name": "serde_json", "kind": None})
        with self.assertRaisesRegex(ValueError, "Forbidden dependency"):
            validate(self.metadata, self.root)

    def test_only_platform_may_depend_on_hf_sys(self):
        for owner in ("hf-domain", "hf-application", "hf-storage", "hf-backends", "hf-interfaces", "hf-host", "hf-fixture-seed"):
            package = self.package(owner)
            package["dependencies"] = [self.sys_edge()]
            with self.subTest(owner=owner), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            package["dependencies"] = []

    def test_hf_sys_edge_policy(self):
        platform = self.package("hf-platform")
        self.package("hf-sys")["dependencies"] = [dict(LIBC)]
        for change in (
            {"source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"path": str(self.root / "crates/hf-domain")}, {"rename": "sys"},
            {"optional": True}, {"target": None}, {"target": "cfg(unix)"},
            {"req": "=0.0.1"}, {"uses_default_features": False}, {"features": ["x"]},
            {"kind": "dev"},
        ):
            platform["dependencies"] = [self.sys_edge(**change)]
            pattern = "Forbidden dependency" if change.get("kind") else "hf-sys requires"
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, pattern):
                validate(self.metadata, self.root)

    def test_hf_sys_permits_one_library_target_and_no_build_script(self):
        self.accepted_edges()
        package = self.package("hf-sys")
        package["targets"].append({"kind": ["bin"], "name": "tool", "src_path": str(self.root / "crates/hf-sys/src/main.rs")})
        with self.assertRaisesRegex(ValueError, "hf-sys permits only"):
            validate(self.metadata, self.root)
        package["targets"][-1] = {"kind": ["test"], "crate_types": ["bin"], "src_path": str(self.root / "crates/hf-sys/tests/t.rs")}
        with self.assertRaisesRegex(ValueError, "hf-sys permits only"):
            validate(self.metadata, self.root)
        package["targets"].pop()
        validate(self.metadata, self.root)
        self.write("crates/hf-sys/build.rs", "fn main() {}\n")
        with self.assertRaisesRegex(ValueError, "hf-sys permits only"):
            validate(self.metadata, self.root)

    def test_unsafe_forms_outside_hf_sys_are_rejected(self):
        self.accepted_edges()
        for index, text in enumerate((
            "fn f() { unsafe { g() } }",
            "unsafe impl Send for X {}",
            "unsafe fn f() {}",
            'unsafe extern "C" { fn g(); }',
            "#[unsafe(no_mangle)]\nfn f() {}",
            "#![allow(unsafe_code)]",
            "#[cfg_attr(test, allow(unsafe_code))]\nmod m {}",
            '#[expect(unsafe_code, reason = "x")]\nlet a = 1;',
        )):
            for relative in ("crates/hf-domain/src/bad.rs", "tools/hf-architecture/src/bad.rs", "crates/hf-sys/tests/bad.rs"):
                path = self.write(relative, text)
                with self.subTest(text=text, relative=relative), self.assertRaisesRegex(ValueError, "outside hf-sys"):
                    validate(self.metadata, self.root)
                path.unlink()

    def test_comments_identifiers_and_strings_naming_unsafe_are_accepted(self):
        self.accepted_edges()
        self.write("crates/hf-domain/src/names.rs", (
            "// unsafe { g() } and allow(unsafe_code) are only text here\n"
            "/* unsafe impl Send for X {} */\n"
            "/// unsafe fn documented\n"
            "fn unsafe_code_name() { let unsafe_value = \"unsafe\"; let _ = unsafe_value; }\n"
            "const NOTE: &str = \"avoid unsafe code\";\n"
        ))
        validate(self.metadata, self.root)

    def test_hf_sys_source_allows_exactly_one_block_and_one_expectation(self):
        self.accepted_edges()
        read_into = self.root / "crates/hf-sys/src/read_into.rs"
        accepted = read_into.read_text(encoding="utf-8")
        for label, extra, pattern in (
            ("second block", "let b = unsafe { h() };\n", "exactly one"),
            ("second expectation", '#[expect(unsafe_code, reason = "x")]\nlet c = 1;\n', "exactly one"),
            ("allow", "#![allow(unsafe_code)]\n", "lint relaxation"),
            ("warn", "#[warn(unsafe_code)]\nmod m {}\n", "lint relaxation"),
            ("unsafe impl", "unsafe impl Send for X {}\n", "Forbidden unsafe form"),
            ("unsafe fn", "unsafe fn f() {}\n", "Forbidden unsafe form"),
            ("unsafe extern", 'unsafe extern "C" { fn g(); }\n', "Forbidden unsafe form"),
            ("no_mangle", "#[unsafe(no_mangle)]\nfn f() {}\n", "Forbidden unsafe form"),
            ("path redirect", '#[path = "x.rs"]\nmod x;\n', "module redirection"),
            ("include", 'include!("x.rs");\n', "module redirection"),
        ):
            read_into.write_text(accepted + extra, encoding="utf-8")
            with self.subTest(label=label), self.assertRaisesRegex(ValueError, pattern):
                validate(self.metadata, self.root)
        read_into.write_text(accepted.replace("unsafe {", "safe {"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exactly one"):
            validate(self.metadata, self.root)

    def test_hf_sys_comments_may_mention_unsafe(self):
        self.accepted_edges()
        path = self.root / "crates/hf-sys/src/read_into.rs"
        path.write_text(path.read_text(encoding="utf-8") + "// unsafe { second } allow(unsafe_code)\n", encoding="utf-8")
        validate(self.metadata, self.root)


if __name__ == "__main__":
    unittest.main()

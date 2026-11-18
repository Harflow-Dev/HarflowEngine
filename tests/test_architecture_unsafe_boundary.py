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


HIDING_LINE = 'let _s = "//"; #[expect(unsafe_code, reason = "x")] let r2 = unsafe { other() };'


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
            HIDING_LINE,
            "let q = '\"'; unsafe { g() }",
            'let _r = r#"//"#; unsafe { g() }',
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

    def test_hf_sys_source_pins_the_single_expectation(self):
        self.accepted_edges()
        read_into = self.root / "crates/hf-sys/src/read_into.rs"
        accepted = read_into.read_text(encoding="utf-8")
        statement = '#[expect(unsafe_code, reason = "reviewed")]\n'
        count = "exactly one unsafe_code token"
        anchor = "directly on the unsafe let statement"
        for label, text, pattern in (
            ("string slashes hiding a second expectation", accepted + HIDING_LINE + "\n", count),
            ("second expectation", accepted + statement + "let c = unsafe { h() };\n", count),
            ("token in a macro call", accepted + "m!(unsafe_code);\n", count),
            ("comment naming the token", accepted + "// unsafe_code note\n", count),
            ("allow", accepted + "#![allow(unsafe_code)]\n", count),
            ("cfg_attr allow", accepted + "#[cfg_attr(all(), allow(unsafe_code))]\nmod m {}\n", count),
            ("no token", accepted.replace(statement, ""), count),
            ("crate-level expectation", accepted.replace(statement, '#![expect(unsafe_code, reason = "x")]\n'), anchor),
            ("expectation on a fn", accepted.replace(statement, "") + '#[expect(unsafe_code, reason = "x")]\nfn f() {}\n', anchor),
            ("expectation not on the statement", accepted.replace("let result", "let other = 1;\n    let result"), anchor),
            ("macro expansion of the statement", accepted.replace(statement, "").replace(
                "let result = unsafe { call() };",
                'macro_rules! raw { ($($t:tt)*) => {{ #[expect(unsafe_code, reason = "x")] let r = unsafe { $($t)* }; r }} }'),
                "Macro definitions"),
            ("path redirect", accepted + '#[path = "x.rs"]\nmod x;\n', "redirection"),
            ("include", accepted + 'include!("x.rs");\n', "redirection"),
        ):
            read_into.write_text(text, encoding="utf-8")
            with self.subTest(label=label), self.assertRaisesRegex(ValueError, pattern):
                validate(self.metadata, self.root)
        read_into.write_text(accepted, encoding="utf-8")
        validate(self.metadata, self.root)

    def test_rustc_configuration_that_could_lower_lints_is_refused(self):
        self.accepted_edges()
        root_manifest = self.root / "Cargo.toml"
        accepted_manifest = root_manifest.read_text(encoding="utf-8")
        toolchain = self.root / "rust-toolchain.toml"
        accepted_toolchain = toolchain.read_text(encoding="utf-8")
        for name in ("config", "config.toml"):
            path = self.write(f"tools/hf-architecture/.cargo/{name}", "[build]\n")
            with self.subTest(cargo=name), self.assertRaisesRegex(ValueError, "In-repo Cargo configuration"):
                validate(self.metadata, self.root)
            path.unlink()
        self.write("target/.cargo/config.toml", "[build]\n")
        validate(self.metadata, self.root)
        for label, text, pattern in (
            ("cargo-features", 'cargo-features = ["x"]\n' + accepted_manifest, "cargo-features"),
            ("profile rustflags", accepted_manifest + '\n[profile.dev]\nrustflags = ["-A", "unsafe_code"]\n', "rustflags"),
        ):
            root_manifest.write_text(text, encoding="utf-8")
            with self.subTest(label=label), self.assertRaisesRegex(ValueError, pattern):
                validate(self.metadata, self.root)
        root_manifest.write_text(accepted_manifest, encoding="utf-8")
        for channel in ("nightly", "stable", "1.99", "nightly-2026-01-01", "1.99.1-beta"):
            toolchain.write_text(f'[toolchain]\nchannel = "{channel}"\n', encoding="utf-8")
            with self.subTest(channel=channel), self.assertRaisesRegex(ValueError, "stable numeric channel"):
                validate(self.metadata, self.root)
        toolchain.write_text("[toolchain]\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "stable numeric channel"):
            validate(self.metadata, self.root)
        toolchain.write_text(accepted_toolchain, encoding="utf-8")
        legacy = self.write("rust-toolchain", "nightly\n")
        with self.assertRaisesRegex(ValueError, "legacy rust-toolchain"):
            validate(self.metadata, self.root)
        legacy.unlink()
        toolchain.unlink()
        with self.assertRaisesRegex(ValueError, "rust-toolchain.toml is required"):
            validate(self.metadata, self.root)
        toolchain.write_text(accepted_toolchain, encoding="utf-8")
        validate(self.metadata, self.root)

    def test_hf_sys_references_are_limited_to_the_init_reader_and_its_test(self):
        self.accepted_edges()
        reader = "crates/hf-platform/src/task_runtime/caller/init_reader.rs"
        test = "crates/hf-platform/tests/caller_system_reader.rs"
        for relative in (reader, test):
            self.write(relative, "use hf_sys::read_into;\n// pub use hf_sys::read_into;\n// macro_rules! m {}\n")
        validate(self.metadata, self.root)
        others = (
            "crates/hf-platform/src/other.rs", "crates/hf-host/src/other.rs",
            "tools/hf-architecture/src/other.rs", "crates/hf-platform/tests/other.rs",
        )
        for text in (
            "use hf_sys::read_into;",
            "fn f() { ::hf_sys::read_into(&[], &mut []); }",
            "extern crate hf_sys as sys;",
            "use hf_sys as sys;",
        ):
            for relative in others:
                path = self.write(relative, text)
                with self.subTest(text=text, relative=relative), self.assertRaisesRegex(ValueError, "outside its allowed readers"):
                    validate(self.metadata, self.root)
                path.unlink()

    def test_allowed_hf_sys_readers_may_not_reexport_or_define_macros(self):
        self.accepted_edges()
        allowed = "crates/hf-platform/src/task_runtime/caller/init_reader.rs"
        for text, pattern in (
            ("pub use hf_sys::read_into;", "re-exported"),
            ("pub(crate) use hf_sys::read_into;", "re-exported"),
            ("pub use {hf_sys as sys};", "re-exported"),
            ("use hf_sys::read_into;\npub(super) use self::read_into as alias;", "re-exported"),
            ("pub extern crate hf_sys;", "re-exported"),
            ("pub(crate) extern crate hf_sys as sys;", "re-exported"),
            ("use hf_sys::read_into;\nmacro_rules! m { () => {}; }", "Macro definitions"),
        ):
            self.write(allowed, text)
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, pattern):
                validate(self.metadata, self.root)
        self.write(allowed, "use hf_sys::read_into;\n")
        validate(self.metadata, self.root)

    def test_hf_platform_redirection_and_inclusion_are_rejected(self):
        self.accepted_edges()
        base = "crates/hf-platform/src/task_runtime"
        reader_path = "task_runtime/caller/init_reader.rs"
        for relative, text in (
            ("crates/hf-platform/src/other.rs", '#[path = "../x.rs"]\nmod x;'),
            ("crates/hf-platform/src/other.rs", '#[path = "/abs/x.rs"]\nmod x;'),
            ("crates/hf-platform/src/other.rs", 'include!("x.rs");'),
            ("crates/hf-platform/tests/other.rs", '#[path = "../src/x.rs"]\nmod x;'),
            ("crates/hf-platform/tests/other.rs", 'include!("x.rs");'),
            ("crates/hf-platform/src/lib.rs", f'#[path = "{reader_path}"]\nmod r;'),
            (f"{base}.rs", '#[path = "caller/init_reader.rs"]\nmod r;'),
            (f"{base}/caller.rs", '#[path = "init_reader.rs"]\nmod r;'),
            ("crates/hf-platform/src/other.rs", f'#[cfg_attr(all(), path = "{reader_path}")]\nmod r;'),
            ("crates/hf-platform/src/other.rs", f'#[path = r#"{reader_path}"#]\nmod r;'),
            ("crates/hf-platform/src/other.rs", 'mod m { #[path = "tests/caller_system_reader.rs"] mod r; }'),
            ("crates/hf-platform/src/other.rs", '#[cfg_attr(test, path = r"../x.rs")]\nmod x;'),
            ("crates/hf-platform/src/lib.rs", f'#[path = "task_runtime/caller/./init_reader.rs"]\nmod r;'),
            ("crates/hf-platform/src/lib.rs", f'#[path = "task_runtime/caller//init_reader.rs"]\nmod r;'),
            ("crates/hf-platform/src/lib.rs", f'#[path = "task_runtime/caller/Init_Reader.rs"]\nmod r;'),
            ("crates/hf-platform/src/lib.rs", '#[path = "\\x2e\\x2e/x.rs"]\nmod r;'),
            ("crates/hf-platform/src/lib.rs", '#[path = "a/b/../../../x.rs"]\nmod r;'),
        ):
            path = self.write(relative, text)
            with self.subTest(relative=relative, text=text), self.assertRaisesRegex(ValueError, "hf-platform"):
                validate(self.metadata, self.root)
            path.unlink()
        for relative, text in (
            ("crates/hf-platform/src/other.rs", '#[path = "other/x.rs"]\nmod x;\nconst T: &str = include_str!("y");'),
            (
                "crates/hf-platform/src/more.rs",
                '// #[path = "../x.rs"]\n/* include!("x.rs") */\n'
                'const A: &str = "#[path = \\"../x.rs\\"]";\nconst B: &str = "include!(x)";',
            ),
            ("crates/hf-platform/tests/other.rs", '#[path = "support/x.rs"]\nmod x;'),
        ):
            self.write(relative, text)
        validate(self.metadata, self.root)

    def test_hf_sys_edge_requires_a_path(self):
        self.package("hf-sys")["dependencies"] = [dict(LIBC)]
        edge = self.sys_edge()
        del edge["path"]
        self.package("hf-platform")["dependencies"] = [edge]
        with self.assertRaisesRegex(ValueError, "hf-sys requires platform-only"):
            validate(self.metadata, self.root)


if __name__ == "__main__":
    unittest.main()

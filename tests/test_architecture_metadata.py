import tempfile
import unittest
from pathlib import Path
import importlib.util

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/check-architecture.py"
SPEC = importlib.util.spec_from_file_location("check_architecture", SCRIPT)
ARCHITECTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ARCHITECTURE)
EDGES = ARCHITECTURE.EDGES
BINS = ARCHITECTURE.BINS
validate = ARCHITECTURE.validate


class ArchitectureMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.packages = []
        for name in EDGES:
            folder = self.root / ("tools" if name in ("hf-architecture", "hf-fixture-seed") else "crates") / name
            folder.mkdir(parents=True)
            manifest = '[lints]\nworkspace = true\n'
            if name == "hf-host":
                manifest = '[package]\nautobins = false\ndefault-run = "harflow-ctl"\n\n' + manifest
            (folder / "Cargo.toml").write_text(manifest, encoding="utf-8")
            targets = [{"kind": ["lib"], "src_path": str(folder / "src/lib.rs")}] if name == "hf-domain" else []
            targets += [{"kind": ["bin"], "name": bin_name, "src_path": str(folder / source)} for bin_name, source in BINS.get(name, {}).items()]
            self.packages.append({
                "id": name,
                "name": name,
                "manifest_path": str(folder / "Cargo.toml"),
                "dependencies": [],
                "targets": targets,
            })
        domain = self.root / "crates/hf-domain/src"
        domain.mkdir()
        (domain / "lib.rs").write_text("pub struct Value;\n", encoding="utf-8")
        self.metadata = {"workspace_members": list(EDGES), "packages": self.packages}

    def tearDown(self):
        self.temporary.cleanup()

    def test_accepts_inventory_with_approved_dependencies_and_lint_inheritance(self):
        self.packages[1]["dependencies"].append({"name": "hf-domain", "kind": None})
        validate(self.metadata, self.root)

    def test_rejects_unapproved_or_non_normal_dependency_edges(self):
        for dependency in ({"name": "std", "kind": None}, {"name": "hf-domain", "kind": "dev"}):
            with self.subTest(dependency=dependency):
                self.packages[0]["dependencies"] = [dependency]
                with self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                    validate(self.metadata, self.root)

    def test_storage_allows_only_exact_bundled_rusqlite_dependency(self):
        storage = next(package for package in self.packages if package["name"] == "hf-storage")
        storage["dependencies"].append({
            "name": "rusqlite", "kind": None, "req": "=0.41.1",
            "uses_default_features": False, "features": ["bundled"],
            "optional": False, "target": None, "rename": None,
        })
        validate(self.metadata, self.root)

    def test_storage_allows_only_exact_storage_owned_sha256_dependency(self):
        storage = next(package for package in self.packages if package["name"] == "hf-storage")
        reviewed = {
            "name": "sha2", "kind": None, "req": "=0.10.9",
            "uses_default_features": False, "features": [],
            "optional": False, "target": None, "rename": None,
        }
        storage["dependencies"] = [reviewed]
        validate(self.metadata, self.root)

        for owner in ("hf-domain", "hf-application", "hf-contracts", "hf-backends", "hf-platform", "hf-interfaces", "hf-host", "hf-architecture"):
            package = next(item for item in self.packages if item["name"] == owner)
            package["dependencies"] = [reviewed]
            with self.subTest(owner=owner), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            package["dependencies"] = []

        for change in (
            {"req": "0.10.9"}, {"req": "=0.10.8"},
            {"uses_default_features": True}, {"features": ["std"]},
            {"features": ["compress"]}, {"kind": "dev"},
            {"kind": "build"}, {"optional": True},
            {"target": 'cfg(unix)'}, {"rename": "digest"},
        ):
            storage["dependencies"] = [{**reviewed, **change}]
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "sha2"):
                validate(self.metadata, self.root)

    def test_rejects_rusqlite_outside_storage_or_with_unreviewed_features(self):
        storage = next(package for package in self.packages if package["name"] == "hf-storage")
        reviewed = {
            "name": "rusqlite", "kind": None, "req": "=0.41.1",
            "uses_default_features": False, "features": ["bundled"],
            "optional": False, "target": None, "rename": None,
        }
        for owner in ("hf-domain", "hf-application", "hf-backends", "hf-platform", "hf-host"):
            package = next(item for item in self.packages if item["name"] == owner)
            package["dependencies"] = [reviewed]
            with self.subTest(owner=owner), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            package["dependencies"] = []

        for change in (
            {"req": "0.41.1"},
            {"uses_default_features": True},
            {"features": ["bundled", "backup"]},
            {"kind": "dev"},
            {"optional": True},
            {"target": "cfg(unix)"},
            {"rename": "sqlite"},
        ):
            dependency = {**reviewed, **change}
            storage["dependencies"] = [dependency]
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "rusqlite"):
                validate(self.metadata, self.root)

    def test_interfaces_allows_only_exact_std_only_serde_json(self):
        interfaces = next(package for package in self.packages if package["name"] == "hf-interfaces")
        reviewed = {
            "name": "serde_json", "kind": None, "req": "=1.0.154",
            "uses_default_features": False, "features": ["std"],
            "optional": False, "target": None, "rename": None,
        }
        interfaces["dependencies"] = [reviewed]
        validate(self.metadata, self.root)

        for owner in ("hf-domain", "hf-application", "hf-storage", "hf-backends", "hf-platform", "hf-host"):
            package = next(item for item in self.packages if item["name"] == owner)
            package["dependencies"] = [reviewed]
            with self.subTest(owner=owner), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            package["dependencies"] = []

        for change in (
            {"req": "1.0.154"},
            {"uses_default_features": True},
            {"features": ["std", "preserve_order"]},
            {"optional": True},
            {"target": "cfg(unix)"},
            {"rename": "json"},
        ):
            interfaces["dependencies"] = [{**reviewed, **change}]
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "serde_json"):
                validate(self.metadata, self.root)

        for kind in ("dev", "build"):
            interfaces["dependencies"] = [{**reviewed, "kind": kind}]
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)

    def test_interfaces_allows_only_exact_alloc_only_base64_dependency(self):
        interfaces = next(package for package in self.packages if package["name"] == "hf-interfaces")
        reviewed = {
            "name": "base64", "kind": None, "req": "=0.22.1",
            "uses_default_features": False, "features": ["alloc"],
            "optional": False, "target": None, "rename": None,
        }
        interfaces["dependencies"] = [reviewed]
        validate(self.metadata, self.root)

        for owner in ("hf-domain", "hf-application", "hf-storage", "hf-backends", "hf-platform", "hf-host"):
            package = next(item for item in self.packages if item["name"] == owner)
            package["dependencies"] = [reviewed]
            with self.subTest(owner=owner), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            package["dependencies"] = []

        for change in (
            {"req": "0.22.1"}, {"req": "=0.22.0"},
            {"uses_default_features": True}, {"features": ["alloc", "std"]},
            {"features": []}, {"optional": True}, {"target": 'cfg(unix)'},
            {"rename": "b64"},
        ):
            interfaces["dependencies"] = [{**reviewed, **change}]
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "base64"):
                validate(self.metadata, self.root)

        for kind in ("dev", "build"):
            interfaces["dependencies"] = [{**reviewed, "kind": kind}]
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)

    def test_rejects_missing_lint_inheritance_and_domain_build_script(self):
        manifest = Path(self.packages[0]["manifest_path"])
        manifest.write_text("[package]\nname = 'hf-domain'\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Workspace lint inheritance"):
            validate(self.metadata, self.root)
        manifest.write_text("[lints]\nworkspace = true\n", encoding="utf-8")
        manifest.parent.joinpath("build.rs").write_text("fn main() {}\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "build scripts are forbidden"):
            validate(self.metadata, self.root)

    def test_platform_os_wrappers_require_exact_macos_only_policy(self):
        platform = next(package for package in self.packages if package["name"] == "hf-platform")
        reviewed = {
            "libproc": ("=0.14.11", []),
            "sysctl": ("=0.7.1", []),
            "nix": ("=0.31.3", ["dir", "fs", "poll", "process", "signal", "socket"]),
        }
        def dependency(name, version, features):
            return {
                "name": name, "kind": None, "req": version,
                "uses_default_features": False, "features": features,
                "optional": False, "target": 'cfg(target_os = "macos")', "rename": None,
            }
        platform["dependencies"] = [dependency(name, *spec) for name, spec in reviewed.items()]
        validate(self.metadata, self.root)

        for name, (version, features) in reviewed.items():
            accepted = dependency(name, version, features)
            changes = [
                {"req": version.lstrip("=")},
                {"req": "=9.9.9"},
                {"uses_default_features": True},
                {"features": features + ["extra"]},
                {"kind": "dev"},
                {"kind": "build"},
                {"optional": True},
                {"target": 'cfg(unix)'},
                {"target": None},
                {"rename": "renamed"},
            ]
            if name == "nix":
                changes.append({"features": ["fs", "poll", "signal"]})
                changes.append({"features": ["fs", "poll", "process", "signal", "socket"]})
                changes.append({"features": ["dir", "fs", "poll", "process", "signal"]})
            for change in changes:
                platform["dependencies"] = [dict(accepted, **change)]
                with self.subTest(name=name, change=change), self.assertRaisesRegex(ValueError, name):
                    validate(self.metadata, self.root)

            outsider = next(package for package in self.packages if package["name"] == "hf-domain")
            outsider["dependencies"] = [accepted]
            with self.subTest(name=name, owner="hf-domain"), self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                validate(self.metadata, self.root)
            outsider["dependencies"] = []

    def test_rejects_custom_domain_targets_and_oversized_rust_files(self):
        domain = self.packages[0]
        domain["targets"].append({"kind": ["bin"], "src_path": str(self.root / "crates/hf-domain/src/main.rs")})
        with self.assertRaisesRegex(ValueError, "conventional src/lib.rs"):
            validate(self.metadata, self.root)
        domain["targets"].pop()
        oversized = self.root / "crates/hf-domain/src/large.rs"
        oversized.write_text("\n".join(f"let value{index} = {index};" for index in range(401)), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "400 substantive lines"):
            validate(self.metadata, self.root)

    def test_binary_inventory_is_exact_and_host_cargo_settings_are_required(self):
        host = next(package for package in self.packages if package["name"] == "hf-host")
        extra = {"kind": ["bin"], "name": "extra", "src_path": str(self.root / "crates/hf-host/src/extra.rs")}
        host["targets"].append(extra)
        with self.assertRaisesRegex(ValueError, "Binary inventory changed"):
            validate(self.metadata, self.root)
        host["targets"].remove(extra)
        removed = host["targets"].pop()
        with self.assertRaisesRegex(ValueError, "Binary inventory changed"):
            validate(self.metadata, self.root)
        host["targets"].append({**removed, "src_path": str(self.root / "crates/hf-host/src/other.rs")})
        with self.assertRaisesRegex(ValueError, "Binary inventory changed"):
            validate(self.metadata, self.root)
        host["targets"].pop()
        host["targets"].append(removed)
        validate(self.metadata, self.root)
        manifest = Path(host["manifest_path"])
        for text in ('[package]\ndefault-run = "harflow-ctl"\n', '[package]\nautobins = false\n',
                     '[package]\nautobins = false\ndefault-run = "harflow-engine-scaffold"\n'):
            manifest.write_text(text + "\n[lints]\nworkspace = true\n", encoding="utf-8")
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, "autobins = false"):
                validate(self.metadata, self.root)

    def test_ctl_code_may_not_name_private_fixture_or_storage_machinery(self):
        for tree in ARCHITECTURE.CTL_TREES:
            folder = self.root / tree
            folder.mkdir(parents=True, exist_ok=True)
            for word in ("fixture", "owner_runtime", "st04", "Qualification", "probe", "initialize_fixture"):
                path = folder / "bad.rs"
                path.write_text(f"// uses {word}\n", encoding="utf-8")
                with self.subTest(tree=tree, word=word), self.assertRaisesRegex(ValueError, "ctl code"):
                    validate(self.metadata, self.root)
                path.unlink()
            (folder / "good.rs").write_text("pub fn inspect() {}\n", encoding="utf-8")
        validate(self.metadata, self.root)

    def test_ctl_module_roots_are_scanned(self):
        for relative in ARCHITECTURE.CTL_ROOTS:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("pub mod fixture;\n", encoding="utf-8")
            with self.subTest(root=relative), self.assertRaisesRegex(ValueError, "ctl code"):
                validate(self.metadata, self.root)
            path.write_text("pub mod state;\n", encoding="utf-8")
        validate(self.metadata, self.root)

    def test_ctl_code_may_not_redirect_modules_or_include_files(self):
        folder = self.root / ARCHITECTURE.CTL_TREES[0]
        folder.mkdir(parents=True, exist_ok=True)
        for text in ('#[path = "../x.rs"]\nmod x;\n', '# [ path = "x.rs" ] mod x;', 'const A: &str = include_str!("x");',
                     'include!("x.rs");', 'include_bytes! ("x")'):
            path = folder / "redirect.rs"
            path.write_text(text, encoding="utf-8")
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, "redirection or file inclusion"):
                validate(self.metadata, self.root)
            path.unlink()
        (folder / "ok.rs").write_text("// a path of its own\npub fn include_all() {}\n", encoding="utf-8")
        validate(self.metadata, self.root)

    def test_host_ctl_storage_paths_are_allowlisted(self):
        folder = self.root / ARCHITECTURE.HOST_CTL_TREE
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / "services.rs"
        path.write_text("let a = hf_storage::SCHEMA_VERSION; let b = hf_storage :: APPLICATION_ID;\n", encoding="utf-8")
        validate(self.metadata, self.root)
        path.write_text(
            "use hf_storage::ReadSession; use hf_storage::Limits; use hf_storage::Error; "
            "use hf_storage::ReadRefusal; use hf_storage::TaskSnapshot;\n", encoding="utf-8")
        validate(self.metadata, self.root)
        for text in ("hf_storage::Store::open(x)", "use hf_storage::{SCHEMA_VERSION};", "use hf_storage;",
                     "hf_storage::SCHEMA_VERSION_EXTRA", "let s = hf_storage::TaskFixture::new();",
                     "hf_storage::ReadSessionExtra", "hf_storage::Store", "hf_storage::RecoveryReadSession"):
            path.write_text(text, encoding="utf-8")
            with self.subTest(text=text), self.assertRaises(ValueError):
                validate(self.metadata, self.root)


if __name__ == "__main__":
    unittest.main()

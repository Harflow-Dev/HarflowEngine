import tempfile
import unittest
from pathlib import Path
import importlib.util

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/check-architecture.py"
SPEC = importlib.util.spec_from_file_location("check_architecture", SCRIPT)
ARCHITECTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ARCHITECTURE)
EDGES = ARCHITECTURE.EDGES
validate = ARCHITECTURE.validate


class ArchitectureMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.packages = []
        for name in EDGES:
            folder = self.root / ("tools" if name == "hf-architecture" else "crates") / name
            folder.mkdir(parents=True)
            (folder / "Cargo.toml").write_text('[lints]\nworkspace = true\n', encoding="utf-8")
            self.packages.append({
                "id": name,
                "name": name,
                "manifest_path": str(folder / "Cargo.toml"),
                "dependencies": [],
                "targets": ([{"kind": ["lib"], "src_path": str(folder / "src/lib.rs")}] if name == "hf-domain" else []),
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
            "nix": ("=0.31.3", ["fs", "poll", "process", "signal"]),
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


if __name__ == "__main__":
    unittest.main()

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

    def test_rejects_missing_lint_inheritance_and_domain_build_script(self):
        manifest = Path(self.packages[0]["manifest_path"])
        manifest.write_text("[package]\nname = 'hf-domain'\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Workspace lint inheritance"):
            validate(self.metadata, self.root)
        manifest.write_text("[lints]\nworkspace = true\n", encoding="utf-8")
        manifest.parent.joinpath("build.rs").write_text("fn main() {}\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "build scripts are forbidden"):
            validate(self.metadata, self.root)

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

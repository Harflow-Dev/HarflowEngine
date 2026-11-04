"""Enforce workspace inventory, dependency edges, lint inheritance and file size."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EDGES = {
    "hf-domain": [], "hf-application": ["hf-domain"], "hf-contracts": [],
    "hf-storage": ["hf-application", "hf-domain", "rusqlite"],
    "hf-backends": ["hf-application", "hf-domain"],
    "hf-platform": ["hf-application", "hf-domain"],
    "hf-interfaces": ["hf-application", "hf-domain"],
    "hf-host": ["hf-application", "hf-domain", "hf-storage", "hf-backends", "hf-platform", "hf-interfaces"],
    "hf-architecture": ["syn"],
}

def validate(metadata, root):
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    if {package["name"] for package in packages} != set(EDGES):
        raise ValueError("Workspace package inventory changed")
    for package in packages:
        for dependency in package["dependencies"]:
            if dependency["name"] not in EDGES[package["name"]] or dependency["kind"] is not None:
                raise ValueError(f"Forbidden dependency: {package['name']} -> {dependency['name']}")
        for dependency in package["dependencies"]:
            if dependency["name"] == "rusqlite" and (
                dependency.get("req") != "=0.41.1"
                or dependency.get("uses_default_features") is not False
                or dependency.get("features") != ["bundled"]
                or dependency.get("optional") is not False
                or dependency.get("target") is not None
                or dependency.get("rename") is not None
            ):
                raise ValueError("rusqlite requires exact reviewed version and bundled-only features")
        manifest_path = Path(package["manifest_path"])
        if package["name"] == "hf-domain":
            expected = (manifest_path.parent / "src/lib.rs").resolve()
            if any(target["kind"] != ["lib"] or Path(target["src_path"]).resolve() != expected for target in package["targets"]):
                raise ValueError("Domain permits only the conventional src/lib.rs library target")
            if (manifest_path.parent / "build.rs").exists():
                raise ValueError("Domain build scripts are forbidden")
        manifest = manifest_path.read_text(encoding="utf-8")
        import tomllib
        if tomllib.loads(manifest).get("lints") != {"workspace": True}:
            raise ValueError(f"Workspace lint inheritance missing: {package['name']}")
    for folder in [root / "crates", root / "tools"]:
        for path in folder.rglob("*.rs"):
            count = sum(bool(line.strip()) and not line.strip().startswith("//") for line in path.read_text(encoding="utf-8").splitlines())
            if count > 400:
                raise ValueError(f"File exceeds 400 substantive lines: {path}")

def main():
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, encoding="utf-8"))
    validate(metadata, ROOT)
    subprocess.run(["cargo", "run", "--locked", "--quiet", "-p", "hf-architecture", "--", str(ROOT / "crates/hf-domain")], cwd=ROOT, check=True)

if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)

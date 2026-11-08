"""Enforce workspace inventory, dependency edges, lint inheritance and file size."""
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EDGES = {
    "hf-domain": [], "hf-application": ["hf-domain"], "hf-contracts": [],
    "hf-storage": ["hf-application", "hf-domain", "rusqlite", "sha2"],
    "hf-backends": ["hf-application", "hf-domain"],
    "hf-platform": ["hf-application", "hf-domain", "libproc", "sysctl", "nix"],
    "hf-interfaces": ["hf-application", "hf-domain", "serde_json", "base64"],
    "hf-host": ["hf-application", "hf-domain", "hf-storage", "hf-backends", "hf-platform", "hf-interfaces"],
    "hf-architecture": ["syn"],
    "hf-fixture-seed": ["hf-domain", "hf-application", "hf-storage", "hf-platform", "hf-interfaces"],
}
BINS = {
    "hf-host": {"harflow-engine-scaffold": "src/main.rs", "harflow-ctl": "src/ctl/main.rs"},
    "hf-architecture": {"hf-architecture": "src/main.rs"},
    "hf-fixture-seed": {"hf-fixture-seed": "src/main.rs"},
}
# Every layer keeps its public ctl code in a `ctl` module: the module root `src/ctl.rs`
# and the tree `src/ctl/`. This is a text screen, not a proof: it rejects the names of
# the private fixture, owner-runtime and storage-qualification machinery, module
# redirection and file inclusion, and any `hf_storage` path in the host ctl tree that is
# not allowlisted. It does not analyse what permitted code does.
CTL_CRATES = ("hf-domain", "hf-application", "hf-platform", "hf-interfaces", "hf-host")
CTL_TREES = [f"crates/{crate}/src/ctl" for crate in CTL_CRATES]
CTL_ROOTS = [f"crates/{crate}/src/ctl.rs" for crate in CTL_CRATES]
CTL_FORBIDDEN = re.compile(r"fixture|owner_runtime|st04|qualification|probe", re.IGNORECASE)
# Wire keys that contracts/v1 requires verbatim and that contain a forbidden word. Only the exact
# quoted occurrence is exempt; widening this set is a deliberate reviewed edit.
CTL_CONTRACT_KEYS = {"time_qualification"}
CTL_REDIRECT = re.compile(r"#\s*!?\s*\[\s*path\b|\binclude(_str|_bytes)?\s*!")
HOST_CTL_TREE = "crates/hf-host/src/ctl"
# Widening this list is a deliberate edit: a new opener or writer must be reviewed here.
HOST_STORAGE_ALLOWED = {
    "hf_storage::SCHEMA_VERSION", "hf_storage::APPLICATION_ID",
    # The read-only session and its typed snapshot: no writer, opener of a mutable store or fixture surface.
    "hf_storage::ReadSession", "hf_storage::Limits", "hf_storage::Error",
    "hf_storage::ReadRefusal", "hf_storage::TaskSnapshot",
}
HOST_STORAGE_PATH = re.compile(r"\bhf_storage\b(?:\s*::\s*\w+)?")

def check_ctl_sources(root):
    files = [root / path for path in CTL_ROOTS if (root / path).is_file()]
    for tree in CTL_TREES:
        files.extend((root / tree).rglob("*.rs"))
    for path in files:
        text = path.read_text(encoding="utf-8")
        for key in CTL_CONTRACT_KEYS:
            text = text.replace(f'"{key}"', '""')
        found = CTL_FORBIDDEN.search(text)
        if found:
            raise ValueError(f"Forbidden name {found.group(0)!r} in ctl code: {path}")
        if CTL_REDIRECT.search(text):
            raise ValueError(f"Module redirection or file inclusion in ctl code: {path}")
    for path in (root / HOST_CTL_TREE).rglob("*.rs"):
        for found in HOST_STORAGE_PATH.findall(path.read_text(encoding="utf-8")):
            if re.sub(r"\s+", "", found) not in HOST_STORAGE_ALLOWED:
                raise ValueError(f"hf_storage path {found!r} is not allowlisted for host ctl code: {path}")

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
            if dependency["name"] == "base64" and (
                package["name"] != "hf-interfaces"
                or dependency.get("req") != "=0.22.1"
                or dependency.get("uses_default_features") is not False
                or dependency.get("features") != ["alloc"]
                or dependency.get("optional") is not False
                or dependency.get("target") is not None
                or dependency.get("rename") is not None
            ):
                raise ValueError("base64 requires interfaces-only exact alloc feature policy")
            if dependency["name"] == "sha2" and (
                package["name"] != "hf-storage"
                or dependency.get("req") != "=0.10.9"
                or dependency.get("uses_default_features") is not False
                or dependency.get("features") != []
                or dependency.get("optional") is not False
                or dependency.get("target") is not None
                or dependency.get("rename") is not None
            ):
                raise ValueError("sha2 requires storage-only exact reviewed version and no features")
            os_wrappers = {
                "libproc": ("=0.14.11", []),
                "sysctl": ("=0.7.1", []),
                # "dir" lets the state observer list directories from a pinned descriptor
                # (nix's dir module is feature-gated; the feature adds no crate).
                "nix": ("=0.31.3", ["dir", "fs", "poll", "process", "signal", "socket", "user"]),
            }
            if dependency["name"] in os_wrappers:
                version, features = os_wrappers[dependency["name"]]
                if (
                    package["name"] != "hf-platform"
                    or dependency.get("req") != version
                    or dependency.get("uses_default_features") is not False
                    or dependency.get("features") != features
                    or dependency.get("optional") is not False
                    or dependency.get("target") != 'cfg(target_os = "macos")'
                    or dependency.get("rename") is not None
                ):
                    raise ValueError(f"{dependency['name']} requires platform-only exact macOS wrapper policy")
            if dependency["name"] == "rusqlite" and (
                dependency.get("req") != "=0.41.1"
                or dependency.get("uses_default_features") is not False
                or dependency.get("features") != ["bundled"]
                or dependency.get("optional") is not False
                or dependency.get("target") is not None
                or dependency.get("rename") is not None
            ):
                raise ValueError("rusqlite requires exact reviewed version and bundled-only features")
        for dependency in package["dependencies"]:
            if dependency["name"] == "serde_json" and (
                package["name"] != "hf-interfaces"
                or dependency.get("req") != "=1.0.154"
                or dependency.get("uses_default_features") is not False
                or dependency.get("features") != ["std"]
                or dependency.get("optional") is not False
                or dependency.get("target") is not None
                or dependency.get("rename") is not None
            ):
                raise ValueError("serde_json requires interfaces-only exact reviewed version and std-only features")
        manifest_path = Path(package["manifest_path"])
        if package["name"] == "hf-domain":
            expected = (manifest_path.parent / "src/lib.rs").resolve()
            if any(target["kind"] != ["lib"] or Path(target["src_path"]).resolve() != expected for target in package["targets"]):
                raise ValueError("Domain permits only the conventional src/lib.rs library target")
            if (manifest_path.parent / "build.rs").exists():
                raise ValueError("Domain build scripts are forbidden")
        manifest = manifest_path.read_text(encoding="utf-8")
        import tomllib
        parsed = tomllib.loads(manifest)
        if parsed.get("lints") != {"workspace": True}:
            raise ValueError(f"Workspace lint inheritance missing: {package['name']}")
        bins = {
            target["name"]: Path(target["src_path"]).resolve().relative_to(manifest_path.parent.resolve()).as_posix()
            for target in package["targets"] if "bin" in target["kind"]
        }
        if bins != BINS.get(package["name"], {}):
            raise ValueError(f"Binary inventory changed: {package['name']}")
        if package["name"] == "hf-host":
            settings = parsed.get("package", {})
            if settings.get("autobins") is not False or settings.get("default-run") != "harflow-ctl":
                raise ValueError("hf-host requires autobins = false and default-run = harflow-ctl")
    for folder in [root / "crates", root / "tools"]:
        for path in folder.rglob("*.rs"):
            count = sum(bool(line.strip()) and not line.strip().startswith("//") for line in path.read_text(encoding="utf-8").splitlines())
            if count > 400:
                raise ValueError(f"File exceeds 400 substantive lines: {path}")
    check_ctl_sources(root)

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

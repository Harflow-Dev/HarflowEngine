"""Shared helpers for the harflow-ctl black-box tests.

The oracle here is Python's own json, os and sqlite3: nothing is shared with the
Rust codec. The binary is located or built the way test_domain_guard.py does it.
"""

import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests" / "fixtures" / "contracts-v1"
TIMEOUT = 30
INVALID_ROOT = (
    "Invalid state root: HARFLOW_STATE_ROOT must be a nonempty absolute path "
    "without NUL characters"
)
APPLICATION_ID = 1_212_560_210
SCRIPTED_ENV = ("HARFLOW_STATE_ROOT", "HARFLOW_TASK_ID", "HARFLOW_DEPTH", "HARFLOW_ROOT_TASK_ID")
_binary = None


def fixture(name):
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def storage_constants():
    """The store schema version, read from the storage crate's source of truth."""
    text = (ROOT / "crates" / "hf-storage" / "src" / "lib.rs").read_text(encoding="utf-8")
    match = re.search(r"pub const SCHEMA_VERSION: u32 = (\d+);", text)
    if match is None:
        raise RuntimeError("SCHEMA_VERSION not found in hf-storage")
    return int(match.group(1))


def binary():
    """Build once, then return the path of the debug harflow-ctl executable."""
    global _binary
    if _binary is None:
        environment = os.environ.copy()
        cargo_home = environment.get("CARGO_HOME")
        if cargo_home:
            environment["PATH"] = os.pathsep.join(
                [str(Path(cargo_home) / "bin"), environment.get("PATH", "")]
            )
        cargo = shutil.which("cargo", path=environment["PATH"])
        if cargo is None:
            raise RuntimeError("cargo is required for the harflow-ctl tests")
        subprocess.run(
            [cargo, "build", "--locked", "-p", "hf-host", "--bin", "harflow-ctl"],
            cwd=ROOT, env=environment, check=True,
        )
        metadata = subprocess.check_output(
            [cargo, "metadata", "--locked", "--format-version", "1", "--no-deps"],
            cwd=ROOT, env=environment, encoding="utf-8",
        )
        target = Path(json.loads(metadata)["target_directory"])
        name = "harflow-ctl.exe" if os.name == "nt" else "harflow-ctl"
        _binary = target / "debug" / name
    return _binary


def real(path):
    """A temporary directory is often under a symlink (macOS /var); resolve it."""
    return os.path.realpath(path)


def run(args, root=None, home=None, cwd=None, env=None, unset=(), timeout=TIMEOUT):
    """Run harflow-ctl with a scrubbed environment and decoded UTF-8 output."""
    environment = {key: value for key, value in os.environ.items() if key not in SCRIPTED_ENV}
    for key in unset:
        environment.pop(key, None)
    if root is not None:
        environment["HARFLOW_STATE_ROOT"] = str(root)
    if home is not None:
        environment["HOME"] = str(home)
        environment["USERPROFILE"] = str(home)
    environment.update(env or {})
    return subprocess.run(
        [str(binary()), *args], cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
        capture_output=True, encoding="utf-8", timeout=timeout,
    )


def one_document(result):
    """Stdout must be exactly one compact JSON line ending in a newline."""
    assert result.stdout.endswith("\n"), repr(result.stdout)
    assert result.stdout.count("\n") == 1, repr(result.stdout)
    return json.loads(result.stdout)


def snapshot(path):
    """Every entry under path, without following links: kind, mode, size, mtime."""
    entries = {}

    def visit(current, key):
        try:
            info = os.lstat(current)
        except PermissionError:
            entries[key] = ("denied",)
            return
        entries[key] = (info.st_mode, info.st_size, info.st_mtime_ns)
        if os.path.isdir(current) and not os.path.islink(current):
            try:
                with os.scandir(current) as scanner:
                    names = sorted(entry.name for entry in scanner)
            except PermissionError:
                return
            for name in names:
                visit(os.path.join(current, name), key + "/" + name)

    visit(str(path), "")
    return entries


class CtlCase(unittest.TestCase):
    """Per-test scratch area: base/home, base/work and base/state (not created)."""

    def setUp(self):
        self.base = real(tempfile.mkdtemp(prefix="harflow-ctl-test-"))
        self.addCleanup(self.cleanup)
        self.home = os.path.join(self.base, "home")
        self.work = os.path.join(self.base, "work")
        self.state = os.path.join(self.base, "state")
        os.mkdir(self.home)
        os.mkdir(self.work)

    def cleanup(self):
        for current, directories, _ in os.walk(self.base):
            for name in directories:
                try:
                    os.chmod(os.path.join(current, name), 0o700)
                except OSError:
                    pass
        shutil.rmtree(self.base, ignore_errors=True)

    def ctl(self, *args, root="default", **options):
        root = self.state if root == "default" else root
        options.setdefault("home", self.home)
        options.setdefault("cwd", self.work)
        return run(list(args), root=root, **options)

    def inspect(self, root=None, **options):
        result = self.ctl("state-inspect", "--json", root=root or self.state, **options)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        return one_document(result)["result"]

    def write(self, name, data, root=None):
        path = os.path.join(root or self.state, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as handle:
            handle.write(data if isinstance(data, bytes) else data.encode("utf-8"))
        return path

    def symlink(self, target, link):
        try:
            os.symlink(target, link)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"symlinks cannot be created here: {error}")


def python_marker(**overrides):
    value = {
        "product": "harflow", "format": "harflow.store", "v": 1,
        "product_version": "0.0.11", "layout_version": 1,
    }
    value.update(overrides)
    return json.dumps(value, sort_keys=True) + "\n"


def identity_text(version, generation="a" * 32):
    return f"harflow-storage-2\n{version}\n{generation}\n"


def database(path, version, application_id=APPLICATION_ID, generation="b" * 32):
    """A real SQLite file in WAL mode with the 0-byte -wal a real engine root keeps."""
    import sqlite3

    connection = sqlite3.connect(path)
    try:
        connection.execute("PRAGMA journal_mode=WAL")
        connection.execute(f"PRAGMA application_id={application_id}")
        connection.execute(f"PRAGMA user_version={version}")
        connection.execute("CREATE TABLE store_identity (singleton INTEGER PRIMARY KEY, generation TEXT)")
        connection.execute("INSERT INTO store_identity VALUES (1, ?)", (generation,))
        connection.commit()
    finally:
        connection.close()
    # Closing this connection cleanly removes -wal and -shm. A real engine root, as
    # initialize_fixture leaves it, keeps a 0-byte -wal (checkpoint-on-close is
    # suppressed) and also an -shm. The fixture models the -wal and leaves -shm out:
    # a root without -shm is a legal state that a reader rebuilds.
    # "xb" refuses a leftover -wal, which would mean a connection outlived the fixture.
    if os.path.exists(path + "-shm"):
        raise RuntimeError("database fixture left a shared-memory file")
    with open(path + "-wal", "xb"):
        pass


WATERMARK_LEN = 8192
WATERMARK_BLOCK = 4096


def watermark_payload(generation, seq):
    """One 64-byte watermark slot payload: magic, generation, seq, zero pad, CRC-32."""
    import struct
    import zlib

    body = b"HFWMARK1" + generation.encode("ascii") + struct.pack("<Q", seq) + bytes(12)
    return body + struct.pack("<I", zlib.crc32(body) & 0xFFFFFFFF)


def watermark_bytes(generation, a=0, b=0):
    """A whole store.watermark: slot A at 0 and slot B at 4096; None leaves a slot zeroed."""
    data = bytearray(WATERMARK_LEN)
    for offset, seq in ((0, a), (WATERMARK_BLOCK, b)):
        if seq is not None:
            data[offset:offset + 64] = watermark_payload(generation, seq)
    return bytes(data)


def rust_root(root, identity_version=None, header_version=None, application_id=APPLICATION_ID,
              generation="a" * 32, watermark=True):
    """A Rust-shaped root: store.identity, a SQLite header and, unless `watermark` is
    False, a seq-0 store.watermark for the marker's generation."""
    current = storage_constants()
    identity_version = current if identity_version is None else identity_version
    header_version = current if header_version is None else header_version
    os.makedirs(root, exist_ok=True)
    with open(os.path.join(root, "store.identity"), "w", encoding="utf-8", newline="") as handle:
        handle.write(identity_text(identity_version, generation))
    database(os.path.join(root, "store.sqlite3"), header_version, application_id, generation)
    if watermark:
        with open(os.path.join(root, "store.watermark"), "xb") as handle:
            handle.write(watermark_bytes(generation))


def journal(root, phase="planned", operation="0123456789abcdef0123456789abcdef", **overrides):
    proposal = {"operation_id": operation, "state_root": root, "root_identity": [1, 2]}
    value = {
        "product": "harflow", "format": "harflow.state-recovery", "v": 1,
        "state_root": root, "phase": phase, "operation_id": operation,
        "proposal": proposal, "backup_path": f"{root}.harflow-backup-{operation}",
        "root_identity": [1, 2], "inventory": {},
    }
    value.update(overrides)
    return json.dumps(value, sort_keys=True) + "\n"

"""Helpers that check a state-inspect recovery proposal against the reference engine.

The oracle is HarflowMCP's own `state_recovery` module, run read-only in a separate
`python3 -I -B` process: its canonical encoding, token decoding and inventory digest.
Nothing here shares code with the Rust codec.
"""

import base64
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from ctl_support import ROOT

REFERENCE_SRC = ROOT.parent / "HarflowMCP" / "src"
OPERATION_ID = re.compile(r"[0-9a-f]{32}")
RECOVERY_KEYS = {"operation_id", "confirmation_token", "backup_path", "phase", "resumable"}
PROPOSAL_KEYS = {
    "state_root", "engine_version", "root_identity", "lock_identity",
    "inventory_sha256", "operation_id", "prior_journal_sha256",
}
# Names this engine leaves out of the inventory on top of the reference's three.
EXTRA_EXCLUDED = ("store.lock", "io.lock", "store.recovery", "store.recovery.tmp", "store.sqlite3-shm")
_PRELUDE = (
    "import json, sys\n"
    "sys.dont_write_bytecode = True\n"
    f"sys.path.insert(0, {str(REFERENCE_SRC)!r})\n"
    "from pathlib import Path\n"
    "from harflow_mcp import state_recovery as reference\n"
)


def reference_available():
    return (REFERENCE_SRC / "harflow_mcp" / "state_recovery.py").is_file()


def run_reference(body, *arguments):
    """Run `body` (which prints one JSON value) against the reference module."""
    done = subprocess.run(
        [sys.executable, "-I", "-B", "-c", _PRELUDE + body, *arguments],
        capture_output=True, encoding="utf-8", timeout=60,
    )
    if done.returncode != 0:
        raise AssertionError(f"reference failed: {done.stderr}")
    return json.loads(done.stdout)


def reference_encode(value):
    """The reference's `_encode`: base64url of its canonical JSON."""
    return run_reference("print(json.dumps(reference._encode(json.loads(sys.argv[1]))))", json.dumps(value))


def reference_decode(token):
    """The reference's `_decode`, or the failure code when it refuses the token."""
    return run_reference(
        "try:\n"
        "    print(json.dumps({'ok': reference._decode(sys.argv[1])}))\n"
        "except Exception as error:\n"
        "    print(json.dumps({'error': getattr(error, 'reason', None) or type(error).__name__}))\n",
        token,
    )


def reference_inventory_digest(root, drop=EXTRA_EXCLUDED):
    """The reference's inventory digest of `root`, without this engine's extra exclusions."""
    return run_reference(
        "inventory = reference._inventory(Path(sys.argv[1]))\n"
        "for name in json.loads(sys.argv[2]):\n"
        "    inventory.pop(name, None)\n"
        "print(json.dumps(reference._digest(inventory)))\n",
        str(root), json.dumps(list(drop)),
    )


def reference_inventory(root, drop=EXTRA_EXCLUDED):
    return run_reference(
        "inventory = reference._inventory(Path(sys.argv[1]))\n"
        "for name in json.loads(sys.argv[2]):\n"
        "    inventory.pop(name, None)\n"
        "print(json.dumps(inventory))\n",
        str(root), json.dumps(list(drop)),
    )


def local_encode(value):
    """Base64url without padding of the canonical JSON (sorted keys, compact, ASCII)."""
    raw = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def local_decode(token):
    padded = token + "=" * (-len(token) % 4)
    return json.loads(base64.urlsafe_b64decode(padded.encode("ascii")))


def assert_no_proposal(case, result):
    case.assertIs(result["recovery_supported"], False, result)
    case.assertNotIn("recovery", result)
    case.assertNotIn("confirmation_token", json.dumps(result))


def assert_proposal(case, result, *, root=None, resumable=False, phase="proposed", digest=True, expected=None):
    """Check the shape of a proposal and its token, against the reference engine when it is checked out.

    The shape and a local canonical-encoding oracle always run; only the comparisons that
    need the reference engine (decode, encode, inventory digest) are skipped without it.
    Returns the decoded token. `digest=False` skips the inventory digest comparison (a
    root whose walk the reference would not complete the same way). `expected` is the
    proposal an unfinished journal holds: a resumed proposal is the journal's own and is
    not measured against the root as it is now.
    """
    with_reference = reference_available()
    root = root or case.state
    case.assertIs(result["recovery_supported"], True, result)
    recovery = result["recovery"]
    case.assertEqual(set(recovery), RECOVERY_KEYS)
    case.assertRegex(recovery["operation_id"], OPERATION_ID)
    case.assertEqual(recovery["backup_path"], f"{result['state_root']}.harflow-backup-{recovery['operation_id']}")
    case.assertEqual(recovery["phase"], phase)
    case.assertIs(recovery["resumable"], resumable)
    token = recovery["confirmation_token"]
    proposal = local_decode(token)
    case.assertEqual(set(proposal), PROPOSAL_KEYS)
    # The token is the canonical encoding of what it holds.
    case.assertEqual(local_encode(proposal), token)
    if with_reference:
        decoded = reference_decode(token)
        case.assertIn("ok", decoded, decoded)
        case.assertEqual(decoded["ok"], proposal)
        case.assertEqual(reference_encode(proposal), token)
    case.assertEqual(proposal["operation_id"], recovery["operation_id"])
    case.assertEqual(proposal["state_root"], result["state_root"])
    if expected is not None:
        case.assertEqual(proposal, expected)
        return proposal
    case.assertEqual(proposal["engine_version"], result["engine_version"])
    info = os.lstat(root)
    case.assertEqual(proposal["root_identity"], [info.st_dev, info.st_ino])
    lock = Path(root) / ".harflow-store.lock"
    if os.path.lexists(lock):
        lock_info = os.lstat(lock)
        case.assertEqual(proposal["lock_identity"], [lock_info.st_dev, lock_info.st_ino])
    else:
        case.assertIsNone(proposal["lock_identity"])
    if digest and not resumable:
        if with_reference:
            case.assertEqual(proposal["inventory_sha256"], reference_inventory_digest(root))
        journal = Path(root) / ".harflow-recovery.json"
        if journal.is_file() and not journal.is_symlink():
            case.assertEqual(proposal["prior_journal_sha256"], hashlib.sha256(journal.read_bytes()).hexdigest())
        else:
            case.assertIsNone(proposal["prior_journal_sha256"])
    return proposal

"""Reject predecessor names outside the explicit allowlist."""
import json
import os
from pathlib import Path

root = Path(__file__).resolve().parents[1]
allowed = json.loads((root / "scripts/legacy-name-allowlist.json").read_text(encoding="utf-8"))
pruned = {".git", "target", "__pycache__"}
for directory, dirnames, filenames in os.walk(root, followlinks=False):
    base = Path(directory)
    relative_base = base.relative_to(root)
    dirnames[:] = [
        name for name in dirnames
        if name not in pruned
        and not (relative_base == Path(".claude") and name == "worktrees")
    ]
    for filename in filenames:
        path = base / filename
        relative = path.relative_to(root)
        if any(part in pruned for part in relative.parts) or not path.is_file():
            continue
        if (b"poly" + b"bridge") in path.read_bytes().lower() and relative.as_posix() not in allowed:
            raise SystemExit(f"Legacy name: {path}")

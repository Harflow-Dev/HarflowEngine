"""Reject predecessor names outside the explicit allowlist."""
import json
from pathlib import Path
root = Path(__file__).resolve().parents[1]
allowed = json.loads((root / "scripts/legacy-name-allowlist.json").read_text(encoding="utf-8"))
for path in root.rglob("*"):
    if any(part in {".git", "target", "__pycache__"} for part in path.relative_to(root).parts) or not path.is_file():
        continue
    if ("poly" + "bridge") in path.read_text(encoding="utf-8").lower() and path.relative_to(root).as_posix() not in allowed:
        raise SystemExit(f"Legacy name: {path}")

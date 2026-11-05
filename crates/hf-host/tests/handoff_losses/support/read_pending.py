"""Independent read-only ownership snapshot for cancellation uncertainty tests."""
import json
import pathlib
import sqlite3
import sys


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: read_pending.py ROOT OPERATION")
    root = pathlib.Path(sys.argv[1]).resolve(strict=True)
    operation = sys.argv[2]
    if len(operation) != 32 or any(char not in "0123456789abcdef" for char in operation):
        raise SystemExit("invalid operation id")
    uri = (root / "store.sqlite3").as_uri() + "?mode=ro"
    connection = sqlite3.connect(uri, uri=True, timeout=1.0)
    try:
        connection.execute("PRAGMA query_only=ON")
        row = connection.execute(
            "SELECT phase,revision,cancelled,release_proof FROM ownership_records WHERE operation=?",
            (operation,),
        ).fetchone()
        print("null" if row is None else json.dumps({
            "phase": row[0], "revision": row[1], "cancelled": row[2],
            "release_proof": row[3],
        }, sort_keys=True))
    finally:
        connection.close()


if __name__ == "__main__":
    main()

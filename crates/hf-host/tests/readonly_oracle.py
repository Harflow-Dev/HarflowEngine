"""Read-only independent oracle for private B2 process qualification tests."""
import json
import pathlib
import sqlite3
import sys


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: readonly_oracle.py ROOT OPERATION")
    root = pathlib.Path(sys.argv[1]).resolve(strict=True)
    operation = sys.argv[2]
    if len(operation) != 32 or any(c not in "0123456789abcdef" for c in operation):
        raise SystemExit("invalid operation id")
    database = root / "store.sqlite3"
    uri = database.as_uri() + "?mode=ro"
    connection = sqlite3.connect(uri, uri=True, timeout=1.0)
    try:
        connection.execute("PRAGMA query_only=ON")
        row = connection.execute(
            "SELECT generation,phase,revision,cancelled,owner,nonce,owner_process,child "
            "FROM ownership_records WHERE operation=?",
            (operation,),
        ).fetchone()
        probe = connection.execute(
            "SELECT value FROM fixture_records WHERE key=?",
            ("r03-b2-probe:" + operation,),
        ).fetchone()
        probe_pid = None if probe is None else bytes(probe[0]).decode("ascii")
        if row is None:
            print("null")
        else:
            print(json.dumps({
                "generation": row[0], "phase": row[1], "revision": row[2],
                "cancelled": row[3], "owner": row[4], "nonce": row[5],
                "owner_process": row[6], "child": row[7],
                "probe_pid": probe_pid,
            }, sort_keys=True))
    finally:
        connection.close()


if __name__ == "__main__":
    main()

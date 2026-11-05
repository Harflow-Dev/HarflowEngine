"""Fault setup: atomically record cancellation against the observed row revision."""
import pathlib
import sqlite3
import sys


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: cancel_pending.py ROOT OPERATION")
    root = pathlib.Path(sys.argv[1]).resolve(strict=True)
    operation = sys.argv[2]
    if len(operation) != 32 or any(char not in "0123456789abcdef" for char in operation):
        raise SystemExit("invalid operation id")
    uri = (root / "store.sqlite3").as_uri()
    connection = sqlite3.connect(uri, timeout=1.0)
    try:
        connection.execute("BEGIN IMMEDIATE")
        changed = connection.execute(
            "UPDATE ownership_records SET cancelled=1,revision=revision+1 "
            "WHERE operation=? AND phase=2 AND revision=2 AND cancelled=0",
            (operation,),
        ).rowcount
        if changed != 1:
            connection.rollback()
            raise SystemExit("expected observed-row cancellation CAS")
        connection.commit()
        print(changed)
    finally:
        connection.close()


if __name__ == "__main__":
    main()

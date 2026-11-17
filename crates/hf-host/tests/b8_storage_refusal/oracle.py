"""Read-only independent SQLite snapshot, never an engine observation API."""
import json
import sqlite3
import sys

with sqlite3.connect("file:" + sys.argv[1] + "/store.sqlite3?mode=ro", uri=True) as db:
    db.execute("PRAGMA query_only=ON")
    db.execute("BEGIN")
    sequence = db.execute("SELECT value FROM commit_sequence WHERE singleton=1").fetchone()[0]
    owner = db.execute("SELECT * FROM ownership_records WHERE operation=?", (sys.argv[2],))
    names = [column[0] for column in owner.description]
    row = owner.fetchone()
    child = next(value for name, value in zip(names, row) if name == "child")
    print(sequence)
    print(child.split(":", 1)[0])
    for table in ("ownership_records", "capture_records", "capture_events"):
        rows = db.execute("SELECT * FROM " + table + " WHERE operation=?", (sys.argv[2],)).fetchall()
        print(table, json.dumps(rows, default=lambda value: value.hex(), sort_keys=True))

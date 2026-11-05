import sqlite3
import sys

database = "file:" + sys.argv[1] + "/store.sqlite3?mode=ro"
connection = sqlite3.connect(database, uri=True, timeout=1.0)
row = connection.execute(
    "SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value "
    "FROM capture_records WHERE operation=?",
    (sys.argv[2],),
).fetchone()
events = connection.execute(
    "SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value "
    "FROM capture_events WHERE operation=? ORDER BY revision",
    (sys.argv[2],),
).fetchall()
owner = connection.execute(
    "SELECT phase,cancelled,release_proof,generation FROM ownership_records WHERE operation=?",
    (sys.argv[2],),
).fetchone()
if row is None:
    raise SystemExit("capture row missing")
kind = "progress" if row[6] == 0 else "terminal"
event_text = ";".join(",".join(str(value) for value in event) for event in events)
print("{}|{}|{}".format(kind, ",".join(str(value) for value in row), event_text))
print("owner:{}:{}:{}:{}".format(owner[0], owner[1], owner[2] or "none", owner[3]))
connection.close()

"""Worlds and commands of the task-artifacts fixtures (generate-artifact-fixtures.py).

The task is `task-artifacts-owner` on the codex backend with a real repository directory. The four
references carry the generator's ids, notes, kinds and provenance; the first two are real files
(one then changed), the other two are deleted and unavailable targets registered with its stat constants.
"""

import json
import shutil
import sqlite3
from pathlib import Path

import bytes_support as policy
from seed_support import task

TASK = "task-artifacts-owner"
STAMP = "2026-10-15T00:00:00+00:00"
SCENARIOS = ("empty", "populated", "next-page", "page-error", "error", "unavailable")
EXCLUDED = {
    "preparing": "ownership_preparing (workflow ownership catalog bootstrap) is not produced by Rust (E6)",
}
CONSTANTS = {"device": 1, "inode": 2, "size": 6, "mtime_ns": 0}
ID = "00000000-0000-0000-0000-%012d"
NAMES = ("report-1.txt", "report-2.txt", f"report-{TASK}.md", "report-4.txt")


def item(number, name, method="verified_process_lineage", **fields):
    entry = {
        "artifact_id": ID % number, "name": name, "target": "present",
        "kind": "report", "note": f" Complete note {number}.\n Preserve whitespace. ",
        "registered_at": STAMP, "provenance_method": method,
    }
    entry.update(fields)
    return entry


def items():
    absent = {"target": "absent", "observation": dict(CONSTANTS)}
    return [
        item(1, NAMES[0]),
        item(2, NAMES[1]),
        item(3, NAMES[2], "engine_output", **absent),
        item(4, NAMES[3], "observed_backend_plan", tool_use_id="fixture-plan-write"),
    ]


def seed(case, register=True, header_valid=True):
    registered = {}
    case.seed("remove_root")
    case.init()
    case.add(TASK, backend="codex", real_workspace=True, started_at=STAMP, finished_at=STAMP)
    repo = Path(case.base) / "repos" / TASK
    if register:
        case.seed("write_artifacts", {"task_id": TASK, "items": items(), "header_valid": header_valid})
        registered.update({name: (repo / "out" / name).stat() for name in (NAMES[0], NAMES[1], NAMES[3])})
        case.seed("change_artifact_target", {"task_id": TASK, "name": NAMES[1]})
    return repo, registered


def stored_revision(case):
    """The manifest revision stored for the task, read from a private copy of the root."""
    copy = Path(case.base) / "revision-copy"
    shutil.rmtree(copy, ignore_errors=True)
    shutil.copytree(case.state, copy)
    connection = sqlite3.connect(copy / "store.sqlite3")
    try:
        return connection.execute("SELECT revision FROM artifact_manifests WHERE task=?", (TASK,)).fetchone()[0]
    finally:
        connection.close()


def listing(case, *args):
    return case.read("task-artifacts", TASK, *args, "--json")


def followup(case, *args):
    def check(cursor):
        done = listing(case, *args, "--cursor", cursor)
        case.assertEqual(done.returncode, 0, done.stderr)
        result = json.loads(done.stdout)["result"]
        case.assertEqual([entry["artifact_id"] for entry in result["items"]], [ID % 3, ID % 4])
        case.assertEqual((result["has_more"], result["next_cursor"]), (False, None))
    return check


def page_rules(case, names):
    """Substitutions of a page that lists `names`: clocks, scratch prefix and port paths."""
    stamps = [("result", "items", i, "availability", "observed_at") for i in range(len(names))]
    return [
        policy.observed(*stamps),
        policy.root_path(str(case.base)),
        policy.artifact_paths("/workspace/repos/" + TASK, *names),
    ]


def run(case, scenario):
    """Seed the world of `scenario`, run its command and return (output, rules)."""
    registered = scenario in ("populated", "next-page", "unavailable")
    repo, registered = seed(case, register=registered, header_valid=scenario != "unavailable")
    if scenario in ("empty", "unavailable"):
        return listing(case), []
    if scenario == "error":
        return listing(case, "--limit", "0"), []
    if scenario == "page-error":
        return listing(case, "--cursor", "not-a-cursor"), []
    cursor = policy.artifact_cursor(
        followup(case, "--limit", "2"), lambda: stored_revision(case))
    if scenario == "populated":
        stats = [registered[name] for name in NAMES[:2]]
        rules = [*page_rules(case, NAMES[:2]), policy.artifact_observations(stats, 0, 1), cursor]
        return listing(case, "--limit", "2"), rules
    first = case.good("task-artifacts", TASK, "--limit", "2")["result"]
    done = listing(case, "--limit", "2", "--cursor", first["next_cursor"])
    return done, [*page_rules(case, NAMES[2:]), policy.artifact_observations([registered[NAMES[3]]], 1)]

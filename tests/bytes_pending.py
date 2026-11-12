"""Worlds and commands of the pending-launch fixtures (generate-pending-launch-fixtures.py).

Receipts are seeded with the generator's values: ids of one repeated character, the fixed
creation stamp, generation `c` x 32, repository `/tmp/harflow-fixture-repo` and the fixture title.
"""

import json

import bytes_support as policy
from pending_support import launch
from seed_support import task

STAMP = "2026-11-10T00:00:00+00:00"
A, B = "a" * 32, "b" * 32
SCENARIOS = (
    "empty", "states", "next-page", "refused", "status-pending", "status-recorded", "status-not-found",
    "status-unknown", "status-refused", "cursor-invalid", "existing-status-pending", "existing-batch-pending",
)
EXCLUDED = {
    "bound": "page-level namespace_bound is unreachable through the binary (the candidate check wins first, recorded ordering delta in docs/pending-launch.md); its document is pinned at interface level by hf-interfaces/tests/ctl_pending_docs.rs",
}


def receipt(case, name=A, state="reserved", **fields):
    spec = launch(name, state, backend="claude", title="Fixture pending launch", created_at=STAMP,
                  repo_path="/tmp/harflow-fixture-repo", **fields)
    case.seed("add_launch", {"spec": spec})


def fresh(case):
    case.seed("remove_root")
    case.init()


def page(case, *args):
    return case.read("task-pending-page", *args, "--json")


def states(case):
    for index, state in enumerate(("reserved", "claimed", "waiting_for_resources", "granted", "not_started")):
        extra = {"resources": {"names": ["fixture"]}} if index > 1 else {}
        if state == "not_started":
            extra["not_started_reason"] = "Fixture pre-spawn refusal"
        receipt(case, str(index + 1) * 32, state, **extra)


def followup(case, *args):
    def check(cursor):
        done = page(case, *args, "--cursor", cursor)
        case.assertEqual(done.returncode, 0, done.stderr)
        result = json.loads(done.stdout)["result"]
        case.assertEqual([item["task_id"] for item in result["items"]], ["5" * 32])
        case.assertEqual((result["has_more"], result["next_cursor"]), (False, None))
    return check


def run(case, scenario):
    """Seed the world of `scenario`, run its command and return (output, extra rules)."""
    fresh(case)
    now = policy.observed(("result", "observed_at"))
    if scenario == "empty":
        return page(case), [now]
    if scenario == "states":
        states(case)
        return page(case), [now]
    if scenario == "next-page":
        states(case)
        first = case.good("task-pending-page", "--limit", "2")["result"]
        done = page(case, "--limit", "2", "--cursor", first["next_cursor"])
        cursor = policy.pending_cursor(case, lambda: case.state, ("result", "next_cursor"),
                                       followup(case, "--limit", "2"))
        return done, [now, cursor]
    if scenario == "refused":
        receipt(case)
        receipt(case, B, refusal={"check": "format"})
        case.seed("add_invalid_launch_name", {"name": ".invalid"})
        return page(case), [now]
    if scenario.startswith("status-"):
        return status(case, scenario), [now]
    if scenario == "cursor-invalid":
        return page(case, "--cursor", "invalid"), []
    return existing(case, scenario), []


def status(case, scenario):
    if scenario != "status-not-found":
        receipt(case, state="spawned" if scenario == "status-unknown" else "reserved",
                **({"refusal": {"check": "format"}} if scenario == "status-refused" else {}))
    if scenario == "status-recorded":
        case.seed("add_task", {"spec": task(A)})
    return case.read("task-pending-status", "--task-id", A, "--json")


def existing(case, scenario):
    receipt(case, state="waiting_for_resources", resources={"names": ["fixture"]})
    if scenario == "existing-status-pending":
        return case.read("status", A, "--json")
    receipt(case, B, "not_started", resources={"names": ["fixture"]}, not_started_reason="Fixture pre-spawn refusal")
    return case.read("status-batch", "--task-ids", A + "," + B, "--json")

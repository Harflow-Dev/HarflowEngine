"""Worlds and commands of the continuation-stop fixtures (generate-continuation-stop-fixtures.py).

The task `task-continuation-stop` is a completed claude task, the root of its own chain; the chain
marker is absent, a stable valid marker attributing the cancel to the task itself, or a dangling
symlink (a fault: unsafe, not verified absence).
"""

import bytes_support as policy
from seed_support import task

TASK = "task-continuation-stop"
STAMP = "2026-10-15T00:00:00+00:00"
NAMES = {
    "ctl-task-status-continuation-not-stopped": ("absent", ("status", TASK)),
    "ctl-task-status-continuation-stopped": ("valid", ("status", TASK)),
    "ctl-task-status-brief-continuation-stopped": ("valid", ("status", TASK, "--brief")),
    "ctl-task-status-batch-continuation-stop": ("valid", ("status-batch", "--task-ids", TASK + ",missing-task")),
    "ctl-task-status-continuation-unknown": ("fault", ("status", TASK)),
}
EXCLUDED = {
    "ctl-continue-chain-stopped.json": "needs task-continue.v1 (E2 write)",
    "ctl-launch-continue-accepted.json": "needs task-continue.v1 (E2 write)",
    "ctl-launch-continue-refused.json": "needs task-continue.v1 (E2 write)",
    "ctl-launch-continue-unknown.json": "needs task-continue.v1 (E2 write)",
}


def run(case, name):
    mode, argv = NAMES[name]
    case.seed("remove_root")
    case.init()
    case.seed("add_task", {"spec": task(TASK, backend="claude", repo_path="/workspace/repo", session_id="",
                                        started_at=STAMP, finished_at=STAMP)})
    spec = {"chain_root": TASK, "mode": "marker" if mode == "valid" else mode}
    if mode == "valid":
        spec["attribution"] = {"kind": "valid", "cancelled_task_id": TASK}
    if mode != "absent":
        case.seed("stop_chain", {"spec": spec})
    batch = argv[0] == "status-batch"
    where = ("tasks", 0) if batch else ("task",)
    clock = policy.observed((*where, "continuation_stop", "observed_at"))
    return case.read(*argv, "--json"), [clock, policy.status_subset("brief" if "--brief" in argv else "status", where)]

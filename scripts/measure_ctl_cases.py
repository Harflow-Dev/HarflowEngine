"""Command lines, the readability check, and the helper processes of the budget harness."""

import base64
import json
import random
import shutil
import subprocess
import threading
import time
from pathlib import Path

from measure_ctl_lib import SEED, clean_env, json_check, run_sample

TASKS = {"ci": 300, "large": 5000, "stats10k": 10000}
WIDE = ["--since", "2000-01-01T00:00:00+00:00", "--before", "2100-01-01T00:00:00+00:00"]
POOL = 20


def task_id(index):
    return f"t-{index:08x}"


def draw(dataset, i):
    """Task id for sample `i`, uniform over the dataset and a pure function of (dataset, i)."""
    return task_id(random.Random(f"{SEED}:{dataset}:{i}").randrange(TASKS[dataset]))


def decode_cursor(text):
    return json.loads(base64.urlsafe_b64decode(text + "=" * (-len(text) % 4)))


def has(*path):
    def test(document):
        for key in path:
            document = document.get(key) if isinstance(document, dict) else None
        return bool(document)

    return test


def ctl_json(ctl, root, argv):
    done = subprocess.run([str(ctl), *argv], env=clean_env(root), stdin=subprocess.DEVNULL,
                          capture_output=True, timeout=60)
    try:
        return done.returncode, json.loads(done.stdout)
    except ValueError:
        return done.returncode, None


class Cursors:
    """Cursors fetched once, outside the timed region, for the pooled cursor cases."""

    def __init__(self, ctl, root, dataset):
        self.pool = [draw(dataset, 10_000_000 + k) for k in range(POOL)]
        _, page = ctl_json(ctl, root, ["task-list-page", "--limit", "50", "--json"])
        self.page = ((page or {}).get("result") or {}).get("next_cursor")
        self.events = {}
        for tid in self.pool:
            _, doc = ctl_json(ctl, root, ["task-events", tid, "--limit", "50", "--json"])
            cursor = ((doc or {}).get("result") or {}).get("older_cursor")
            if cursor and ((doc or {}).get("result") or {}).get("has_older"):
                self.events[tid] = cursor


def ops(dataset, cursors=None):
    """name -> (argv(i), checker). The four gated operations, then the S6 query additions."""
    result = lambda *k: has("result", *k)  # noqa: E731
    table = {
        "version-info": (lambda i: ["version-info", "--json"],
                         json_check("harflow.version-info", None)),
        "status": (lambda i: ["status", draw(dataset, i), "--json"],
                   json_check(test=has("task", "task_id"))),
        "list-page": (lambda i: ["task-list-page", "--limit", "50", "--json"],
                      json_check(test=result("items"))),
        "event-page": (lambda i: ["task-events", draw(dataset, i), "--limit", "50", "--json"],
                       json_check(test=result("events"))),
    }
    if cursors is not None:
        ids = sorted(cursors.events)
        table["list-filter"] = (
            lambda i: ["task-list-page", "--limit", "50", "--session-id", f"s{i % 20}", "--json"],
            json_check(test=result("items")))
        table["list-cursor"] = (
            lambda i: ["task-list-page", "--limit", "50", "--cursor", cursors.page, "--json"],
            json_check(test=result("items")))
        table["event-cursor"] = (
            lambda i: ["task-events", ids[i % len(ids)], "--limit", "50", "--cursor",
                       cursors.events[ids[i % len(ids)]], "--json"],
            json_check(test=result("events")))
        table["event-oldest"] = (
            lambda i: ["task-events", draw(dataset, i), "--from", "oldest", "--limit", "50",
                       "--json"], json_check(test=result("events")))
    return table


def components(dataset):
    """Recorded-only commands on one store: name -> (argv(i), checker)."""
    ids = ",".join(task_id(k * 7 % TASKS[dataset]) for k in range(20))
    return {
        "status-batch-20": (lambda i: ["status-batch", "--task-ids", ids, "--json"],
                            json_check(test=has("tasks"))),
        "list": (lambda i: ["list", "--json"], json_check(test=has("tasks"))),
        "task-artifacts": (lambda i: ["task-artifacts", draw(dataset, i), "--json"],
                           json_check(test=has("result", "state"))),
        "interaction-events": (lambda i: ["interaction-events", "--limit", "100", "--json"],
                               json_check(test=has("result", "events"))),
        "recovery-activity": (lambda i: ["recovery-activity", draw(dataset, i), "--json"],
                              json_check(test=has("recovery_activity"))),
        "statistics-page": (lambda i: ["statistics-page", *WIDE, "--limit", "100", "--json"],
                            json_check(test=has("result", "generation"))),
        "task-prompt": (lambda i: ["task-prompt", draw(dataset, i), "--json"],
                        json_check(test=has("result", "prompt", "state"))),
    }


def readability(ctl, roots):
    """Every gated and recorded command family must answer on the generated ci store."""
    failures = []
    for state, root in roots.items():
        cursors = Cursors(ctl, root, "ci")
        usable = bool(cursors.page) and bool(cursors.events)
        if not usable:
            failures.append(f"{state}: no list or event cursors to sample "
                            f"(list cursor {cursors.page!r}, event cursors {len(cursors.events)})")
        listing = {**ops("ci", cursors if usable else None), **components("ci")}
        for name, (argv, check) in listing.items():
            for i in range(3):
                code, doc = ctl_json(ctl, root, argv(i))
                err = check(code, json.dumps(doc).encode() if doc is not None else b"")
                if err:
                    failures.append(f"{state}/{name}[{i}]: {err} {argv(i)}")
        code, doc = ctl_json(ctl, root, ["state-inspect", "--json"])
        if code or not (doc or {}).get("result", {}).get("reason") == "compatible":
            failures.append(f"{state}/state-inspect: {doc}")
        code, doc = ctl_json(ctl, root, ["list", "--json"])
        if code or len((doc or {}).get("tasks", [])) != 300 or (doc or {}).get("refused_tasks"):
            failures.append(f"{state}/list: expected 300 tasks and none refused")
    return failures


def harness_child(measure_bin, root, stop, role, ckpt="0"):
    """The existing ignored harness entry (`child`) as a helper process: `hold` or `writer`."""
    env = {"HF_MEASURE_OP": role, "HF_MEASURE_ROOT": str(root), "HF_MEASURE_KIND": "0",
           "HF_MEASURE_TASK": "0", "HF_MEASURE_STOP": str(stop), "HF_MEASURE_CKPT": ckpt,
           "PATH": "/usr/bin:/bin"}
    return subprocess.Popen(
        [str(measure_bin), "--exact", "child", "--ignored", "--nocapture", "--test-threads", "1"],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)


class Holder:
    """A read-only connection kept open, as a live owner would (amendment 2)."""

    def __init__(self, measure_bin, root, work):
        self.stop = Path(work) / "stop-holder"
        self.stop.unlink(missing_ok=True)
        self.child = harness_child(measure_bin, root, self.stop, "hold")
        for line in self.child.stdout:
            if "HFH READY" in line:
                return
        raise RuntimeError("the holder never became ready")

    def close(self):
        self.child.kill()
        self.child.wait()


def contention(ctl, measure_bin, source, work, checkpoints, per_reader, dataset="ci"):
    """4 concurrent cold readers over a writer on a copy of `source`; returns (samples, writer)."""
    scratch = Path(work) / f"{source.name}-contended"
    shutil.rmtree(scratch, ignore_errors=True)
    subprocess.run(["cp", "-cR", str(source), str(scratch)], check=True)
    stop = Path(work) / "stop-writer"
    window = Path(f"{stop}.window")
    stop.unlink(missing_ok=True)
    window.unlink(missing_ok=True)
    writer = harness_child(measure_bin, scratch, stop, "writer", "1" if checkpoints else "0")
    time.sleep(2)
    window.write_text("x")
    kinds = ops(dataset)
    mix = [kinds["status"], kinds["list-page"], kinds["event-page"]]
    out = [[] for _ in range(4)]

    def reader(slot):
        for i in range(per_reader):
            argv, check = mix[i % 3]
            n = slot * per_reader + i
            out[slot].append(run_sample([str(ctl), *argv(n)], clean_env(scratch), check))

    began = time.perf_counter()
    threads = [threading.Thread(target=reader, args=(s,)) for s in range(4)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    window_s = round(time.perf_counter() - began, 2)
    window.unlink(missing_ok=True)
    stop.write_text("x")
    timed_out = False
    try:
        text, _ = writer.communicate(timeout=120)
    except subprocess.TimeoutExpired:
        timed_out = True
        writer.kill()
        text, _ = writer.communicate()
    shutil.rmtree(scratch, ignore_errors=True)
    stop.unlink(missing_ok=True)
    reports = [ln[ln.index("HFW ") + 4:] for ln in text.splitlines() if "HFW " in ln]
    stats = json.loads("{" + reports[0] + "}") if len(reports) == 1 else {}
    problems = ["writer did not stop within 120 s and was killed"] if timed_out else []
    if writer.returncode != 0:
        problems.append(f"writer exit status {writer.returncode}")
    if len(reports) != 1:
        problems.append(f"expected one HFW report, found {len(reports)}")
    if stats and stats.get("mode") != "store":
        problems.append(f"writer mode {stats.get('mode')!r}, expected 'store'")
    if stats and not stats.get("window_commits"):
        problems.append("no writer commits inside the reader window")
    if stats and checkpoints and not stats.get("checkpoints"):
        problems.append("checkpoint variant ran no checkpoint")
    stats["reader_window_s"] = window_s
    stats["valid"] = not problems
    stats["problems"] = problems
    stats["diagnostics"] = {"exit_status": writer.returncode, "writer_stdout_tail": text[-2000:]}
    return [s for slot in out for s in slot], stats

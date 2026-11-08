"""Shared pieces of the harflow-ctl budget harness: sampling, statistics, load rules, env.txt.

The protocol is docs/store-read-measurement.md and its S6 amendment. Nothing here retunes a
number: percentiles are nearest-rank on the sorted samples, no outlier is dropped, and failed
samples stay in the distribution with the time at which they failed.
"""

import json
import math
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

SEED = 0x48465332
LOAD_LIMIT = 5.5
ATTEMPTS = 3
TIMEOUT_S = 30
PAUSE_S = 10  # between attempts of a load-flagged batch


def clean_env(root):
    """The environment of one harflow-ctl process: no agent markers, the root named."""
    env = {"PATH": "/usr/bin:/bin", "HOME": os.environ.get("HOME", "/"), "LC_ALL": "C"}
    if root is not None:
        env["HARFLOW_STATE_ROOT"] = str(root)
    return env


def run_sample(argv, env, check):
    """Spawn to exit wall time of one process, and whether it answered as `check` expects.

    `check(returncode, stdout)` returns None for success or an error-kind string.
    """
    start = time.perf_counter_ns()
    try:
        done = subprocess.run(
            argv, env=env, stdin=subprocess.DEVNULL, capture_output=True, timeout=TIMEOUT_S
        )
        wall_ns = time.perf_counter_ns() - start
        code, out = done.returncode, done.stdout
    except subprocess.TimeoutExpired:
        wall_ns = time.perf_counter_ns() - start
        code, out = -1, b""
    kind = "timeout" if code == -1 else check(code, out)
    return {"wall_us": wall_ns // 1000, "err": kind or "", "bytes": len(out)}


def json_check(expect_format="harflow.ctl", test=None):
    """A checker: exit 0, parseable JSON of the format, and `test(document)` truthy."""

    def check(code, out):
        try:
            document = json.loads(out)
        except ValueError:
            return error_kind(code, None) if code else "bad_output"
        if code != 0:
            return error_kind(code, document)
        if document.get("format") != expect_format:
            return "bad_format"
        return None if test is None or test(document) else "unexpected_content"

    return check


def error_kind(code, document):
    error = (document or {}).get("error") or {}
    return str(error.get("code") or f"exit_{code}")


def nearest_rank(ordered, percent):
    return ordered[max(0, math.ceil(percent / 100 * len(ordered)) - 1)]


def summarize(samples):
    walls = sorted(s["wall_us"] for s in samples)
    kinds = {}
    for sample in samples:
        if sample["err"]:
            kinds[sample["err"]] = kinds.get(sample["err"], 0) + 1
    if not walls:
        return {"n": 0}
    ms = lambda us: round(us / 1000, 3)  # noqa: E731
    return {
        "n": len(walls),
        "p50_ms": ms(nearest_rank(walls, 50)),
        "p95_ms": ms(nearest_rank(walls, 95)),
        "p99_ms": ms(nearest_rank(walls, 99)),
        "max_ms": ms(walls[-1]),
        "failures": sum(kinds.values()),
        "failure_kinds": kinds,
    }


def other_jobs():
    """Count of cargo and rustc processes visible now."""
    out = subprocess.run(["/bin/ps", "-axo", "comm="], capture_output=True, text=True).stdout
    return sum(1 for line in out.splitlines() if Path(line.strip()).name in ("cargo", "rustc"))


def load_now():
    return {"load": [round(v, 2) for v in os.getloadavg()], "jobs": other_jobs(), "unix": int(time.time())}


class Recorder:
    """Writes raw samples, batches.jsonl and keeps the per-batch results for the summary."""

    def __init__(self, run_dir, label):
        """`run_dir` is new: created exclusively by `new_run_dir`, never reused."""
        self.out = Path(run_dir)
        self.label = label
        (self.out / "raw").mkdir()
        self.batches = open(self.out / "batches.jsonl", "x", encoding="utf-8")
        self.results = {}

    def note(self, record):
        self.batches.write(json.dumps(record, sort_keys=True) + "\n")
        self.batches.flush()

    def batch(self, name, produce, extra=None):
        """Runs `produce()` (a list of samples) up to three times until load is clean.

        Every attempt is kept. The result is the first unflagged attempt, otherwise the last
        attempt, marked load-compromised.
        """
        chosen = None
        for attempt in range(1, ATTEMPTS + 1):
            start = load_now()
            samples = produce()
            end = load_now()
            flagged = max(start["load"][0], end["load"][0]) > LOAD_LIMIT
            path = self.out / "raw" / f"{self.label}.{name}.a{attempt}.jsonl"
            with open(path, "x", encoding="utf-8") as handle:
                for index, sample in enumerate(samples):
                    handle.write(json.dumps({"i": index, **sample}) + "\n")
            self.note({
                "label": self.label, "batch": name, "attempt": attempt, "start": start,
                "end": end, "flagged": flagged, "n": len(samples), "raw": path.name,
                **(extra() if extra else {}),
            })
            chosen = {"attempt": attempt, "flagged": flagged, "samples": samples}
            if not flagged:
                break
            if attempt < ATTEMPTS:
                time.sleep(PAUSE_S)
        chosen["load_compromised"] = chosen["flagged"]
        chosen["summary"] = summarize(chosen["samples"])
        self.results[name] = chosen
        return chosen


def new_run_dir(parent, label):
    """A fresh run directory `<UTC timestamp>-<label>` under `parent`; refuses to reuse one."""
    parent = Path(parent)
    parent.mkdir(parents=True, exist_ok=True)
    run = parent / f"{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}-{label}"
    run.mkdir()  # FileExistsError if it exists: earlier evidence is never touched
    return run


LOCK_NAME = ".measure.lock"


def acquire_lock(parent):
    """Serialize runs under one parent: an exclusive lock file holding the pid.

    Refuses when it exists, naming the holder. A stale lock is never broken automatically.
    """
    parent = Path(parent)
    parent.mkdir(parents=True, exist_ok=True)
    lock = parent / LOCK_NAME
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
    except FileExistsError:
        holder = lock.read_text(encoding="utf-8").strip() if lock.exists() else "unknown"
        sys.exit(f"another run holds {lock}: {holder}\nIf that run is dead, removing the lock "
                 "file is a manual step; it is never broken automatically.")
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(f"pid {os.getpid()}\n")
    return lock


def name_lock(lock, run):
    Path(lock).write_text(f"pid {os.getpid()} run {Path(run).name}\n", encoding="utf-8")


def prior_runs(parent, run):
    """Earlier sibling run directories: (eligible ones, all others incl. killed with no summary)."""
    eligible, others = [], []
    for sibling in sorted(Path(parent).iterdir()):
        if not sibling.is_dir() or sibling.name >= Path(run).name:
            continue
        try:
            summary = json.loads((sibling / "summary.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            others.append({"run": sibling.name, "why": "no readable summary (killed or partial)"})
            continue
        if summary.get("gate_eligible") is True:
            eligible.append(sibling.name)
        else:
            others.append({"run": sibling.name, "why": "not gate-eligible"})
    return eligible, others


def linked_sqlite_version(measure_bin):
    """The version string the linked library reports, via the harness child entry."""
    env = {"HF_MEASURE_OP": "sqlite_version", "HF_MEASURE_ROOT": "/", "HF_MEASURE_KIND": "0",
           "HF_MEASURE_TASK": "0", "PATH": "/usr/bin:/bin"}
    try:
        done = subprocess.run(
            [str(measure_bin), "--exact", "child", "--ignored", "--nocapture", "--test-threads",
             "1"], env=env, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "unavailable: timeout"
    for line in done.stdout.splitlines():
        if "HFS " in line:
            return line[line.index("HFS ") + 4:].strip()
    return f"unavailable (exit {done.returncode})"


def digest(path):
    import hashlib

    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def capture(argv):
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=60)
        return (done.stdout + done.stderr).strip()
    except (OSError, subprocess.SubprocessError) as error:
        return f"unavailable: {error}"


def write_env(out_dir, repo, commands, store_dir, measure_bin, ctl):
    """env.txt per protocol: the machine, the toolchain, the build and the exact commands."""
    git = lambda *a: capture(["git", "-C", str(repo), *a])  # noqa: E731
    usage = shutil.disk_usage(store_dir)
    lines = [
        f"date_utc: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}",
        f"git_head: {git('rev-parse', 'HEAD')}",
        f"git_status_short:\n{git('status', '--short')}",
        f"rustc -Vv:\n{capture(['rustc', '-Vv'])}",
        f"cargo -V: {capture(['cargo', '-V'])}",
        "build: release (cargo build --release --locked -p hf-host --bin harflow-ctl), "
        "declared, not observed",
        *[f"binary {label}: {path} size {Path(path).stat().st_size} sha256 {digest(path)}"
          for label, path in (("ctl", ctl), ("measure-bin", measure_bin))],
        f"uname -a: {capture(['uname', '-a'])}",
        f"sw_vers:\n{capture(['sw_vers'])}",
        f"cpu: {capture(['sysctl', '-n', 'machdep.cpu.brand_string'])}, "
        f"{capture(['sysctl', '-n', 'hw.ncpu'])} cores",
        f"ram_bytes: {capture(['sysctl', '-n', 'hw.memsize'])}",
        f"store_dir_filesystem: {capture(['df', str(store_dir)]).splitlines()[-1]}",
        f"free_bytes: {usage.free}",
        f"sqlite_version_linked: {linked_sqlite_version(measure_bin)}",
        f"python: {sys.version.split()[0]} {platform.platform()}",
        f"load_at_start: {load_now()}",
        "commands:",
        *[f"  {c}" for c in commands],
    ]
    # The rusqlite crate versions come from the lockfile.
    lock = repo / "Cargo.lock"
    if lock.exists():
        text = lock.read_text(encoding="utf-8")
        for name in ("rusqlite", "libsqlite3-sys"):
            at = text.find(f'name = "{name}"')
            if at >= 0:
                lines.append(f"{name}: {text[at:].splitlines()[1].strip()}")
    (Path(out_dir) / "env.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")

#!/usr/bin/env python3
"""Budget harness for the release harflow-ctl (E1 slice S6, work item W4).

Protocol: docs/store-read-measurement.md, including its S6 amendment. Every sample is one cold
harflow-ctl process timed from spawn to exit. The dataset directories are produced by the
ignored `generate`, `tail` and `generate_prompt` entries of the hf-storage `read_measure`
test binary; the same binary supplies the contention writer and the holder.

Gated: the ci dataset in the wal, ckpt and tail states, against the budget table. Everything
else is recorded. Output goes to --out, which must be outside the repository.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

import measure_ctl_cases as cases
from measure_ctl_lib import (Recorder, acquire_lock, clean_env, name_lock, new_run_dir,
                             prior_runs, run_sample, write_env)

REPO = Path(__file__).resolve().parents[1]
STATES = ("wal", "ckpt", "tail")
BUDGET_MS = {"version-info": 50, "status": 50, "list-page": 100, "event-page": 100}
PARENT = {"list-filter": "list-page", "list-cursor": "list-page",
          "event-cursor": "event-page", "event-oldest": "event-page"}
GATED = tuple(BUDGET_MS)
GATE_SAMPLES = 200


def parse():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--ctl", required=True, help="path to the release harflow-ctl")
    ap.add_argument("--data", required=True, help="directory holding ci-wal, ci-ckpt, ci-tail, ...")
    ap.add_argument("--out", required=True, help="parent of the run directories, outside the repository; "
                    "each run creates <UTC timestamp>-<label> in it, exclusively")
    ap.add_argument("--measure-bin", required=True,
                    help="the hf-storage read_measure test binary (writer and holder)")
    ap.add_argument("--label", required=True, help="run label; 'smoke' for a smoke run")
    ap.add_argument("--samples", type=int, default=200)
    ap.add_argument("--readers", type=int, default=50, help="samples per contention reader")
    ap.add_argument("--only", default="", help="comma list of groups: gated,holder,additions,"
                    "contention,large,components,null; default all present")
    ap.add_argument("--check-only", action="store_true", help="run the readability check and stop")
    args = ap.parse_args()
    if args.samples < 1:
        ap.error("--samples must be at least 1")
    if args.readers < 1:
        ap.error("--readers must be at least 1")
    return args


def sampler(ctl, root, argv, check, n):
    def produce():
        return [run_sample([str(ctl), *argv(i)], clean_env(root), check) for i in range(n)]

    return produce


def gate(name, result, case_ops):
    summary = result["summary"]
    limit = BUDGET_MS.get(PARENT.get(case_ops, case_ops))
    if limit is None:
        return None
    reason = []
    if summary.get("n") != GATE_SAMPLES:
        reason.append("n != 200")
    if result["load_compromised"]:
        reason.append("load-compromised")
    if summary.get("failures"):
        reason.append("failures")
    p95 = summary.get("p95_ms")
    if p95 is None:
        reason.append("no p95 (no samples)")
    elif p95 >= limit:
        reason.append("p95 over limit")
    return {"limit_ms": limit, "p95_ms": p95,
            "result": "pass" if not reason else "fail", "reasons": reason}


def main():
    args = parse()
    ctl, data, parent = Path(args.ctl), Path(args.data), Path(args.out).resolve()
    if REPO in [parent, *parent.parents]:
        sys.exit("--out must be outside the repository")
    only = set(filter(None, args.only.split(",")))
    want = lambda group: not only or group in only  # noqa: E731
    roots = {s: data / f"ci-{s}" for s in STATES}
    missing = [str(r) for r in roots.values() if not r.is_dir()]
    if missing:
        sys.exit(f"missing datasets: {missing}")
    lock = acquire_lock(parent)
    try:
        run(args, ctl, data, parent, lock, only, want, roots)
    finally:
        lock.unlink(missing_ok=True)


def run(args, ctl, data, parent, lock, only, want, roots):
    out = new_run_dir(parent, args.label)
    name_lock(lock, out)
    prior = prior_runs(parent, out)
    print("run directory:", out)
    write_env(out, REPO, [" ".join(sys.argv)], data, args.measure_bin, ctl)
    rec = Recorder(out, args.label)
    rec.missing = []  # requested recorded groups whose data directory is absent

    failures = cases.readability(ctl, roots)
    rec.note({"label": args.label, "check": "readability", "failures": failures})
    if failures:
        print("readability check failed; refusing to sample:\n  " + "\n  ".join(failures))
        sys.exit(2)
    print("readability check passed on", ", ".join(roots))
    if args.check_only:
        return

    n = args.samples
    tool = args.measure_bin
    if want("null"):
        rec.batch("null", lambda: [run_sample(["/usr/bin/true"], clean_env(None), lambda c, o: None
                                              if c == 0 else "exit") for _ in range(n)])
    for state in STATES:
        cur = cases.Cursors(ctl, roots[state], "ci")
        table = cases.ops("ci", cur)
        for group, names in (("gated", GATED), ("additions", tuple(PARENT))):
            if not want(group):
                continue
            for name in names:
                argv, check = table[name]
                rec.batch(f"ci-{state}.{name}", sampler(ctl, roots[state], argv, check, n))
        if want("holder"):
            held = cases.Holder(tool, roots[state], out)
            try:
                for name in GATED:
                    argv, check = table[name]
                    rec.batch(f"holder.ci-{state}.{name}",
                              sampler(ctl, roots[state], argv, check, n))
            finally:
                held.close()
    if want("contention"):
        for checkpoints in (False, True):
            name = f"contention.ci-wal.ckpt{int(checkpoints)}"
            stats = {}

            def produce():
                samples, stats["writer"] = cases.contention(
                    ctl, tool, roots["wal"], out, checkpoints, args.readers)
                return samples

            def note_writer():
                # The complete report: commits, errors, checkpoints, seconds, latency
                # percentiles, window counts, problems and the output tail.
                return {"writer": dict(stats.get("writer", {}))}

            rec.batch(name, produce, note_writer)
            rec.results[name]["writer"] = stats["writer"]
    if want("large"):
        for state in ("wal", "ckpt"):
            root = data / f"large-{state}"
            if not root.is_dir():
                rec.missing.append(f"large-{state}")
                continue
            table = cases.ops("large")
            for name in GATED:
                argv, check = table[name]
                rec.batch(f"large-{state}.{name}", sampler(ctl, root, argv, check, n))
    if want("components"):
        run_components(rec, ctl, data, roots["wal"], n)
    eligible = gate_eligibility(args, only) + (
        [f"missing recorded data: {rec.missing}"] if rec.missing else [])
    summarize_run(rec, out, ctl, eligible, args, only, prior)


def gate_eligibility(args, only):
    """Why this run cannot gate; empty when it can (full case set, 200 samples, not smoke)."""
    why = []
    if args.label == "smoke":
        why.append("label is 'smoke'")
    if only:
        why.append("--only filtering")
    if args.samples != GATE_SAMPLES:
        why.append(f"{args.samples} samples per case, {GATE_SAMPLES} required")
    return why


def run_components(rec, ctl, data, wal, n):
    ps = lambda: run_sample(  # noqa: E731
        ["/bin/ps", "-axo", "pid=,ppid="], {"LC_ALL": "C"},
        lambda c, o: None if c == 0 else "exit")
    rec.batch("component.ps", lambda: [ps() for _ in range(n)])
    for name, (argv, check) in cases.components("ci").items():
        rec.batch(f"component.ci-wal.{name}", sampler(ctl, wal, argv, check, n))
    for dirname in ("prompt4m", "prompt4m-inline"):
        prompt = data / dirname
        if not prompt.is_dir():
            rec.missing.append(dirname)
        else:
            check = cases.components("ci")["task-prompt"][1]
            rec.batch(f"component.{dirname}.task-prompt", sampler(
                ctl, prompt, lambda i: ["task-prompt", "t-00000000", "--json"], check, n))
    stats = data / "stats10k-ckpt"
    if not stats.is_dir():
        rec.missing.append("stats10k-ckpt")
    else:
        argv, check = cases.components("stats10k")["statistics-page"]
        rec.batch("component.stats10k.statistics-page", sampler(ctl, stats, argv, check, n))


def static_components(ctl):
    otool = subprocess.run(["otool", "-L", str(ctl)], capture_output=True, text=True).stdout
    return {"binary_bytes": Path(ctl).stat().st_size, "otool_L": otool.strip().splitlines()[1:]}


def summarize_run(rec, out, ctl, ineligible, args, only, prior):
    summary = {"label": rec.label, "samples": args.samples, "only": sorted(only),
               "evidence": False, "missing_data": rec.missing, "components_static": static_components(ctl), "cases": {}}
    for name, result in rec.results.items():
        entry = {"attempt": result["attempt"], "load_compromised": result["load_compromised"],
                 **result["summary"]}
        kind = name.rsplit(".", 1)[-1]
        gated = name.startswith("ci-") and kind in GATED
        recorded = name.startswith("ci-") and kind in PARENT
        if gated or recorded:
            entry["budget"] = gate(name, result, kind)
            entry["gated"] = gated
        if "writer" in result:
            entry["writer"] = result["writer"]
            entry["contention_valid"] = result["writer"].get("valid", False)
            entry.update({k: result["writer"].get(k) for k in ("mode", "commits", "window_commits")})
            if not entry["contention_valid"]:
                entry["contention"] = "invalid: not contention evidence"
        summary["cases"][name] = entry
    # The complete gated set: four operations x three states on ci, each at exactly 200 samples.
    wanted = {f"ci-{s}.{op}" for s in STATES for op in GATED}
    present = {n for n, e in summary["cases"].items() if n in wanted and e.get("n") == GATE_SAMPLES}
    why = list(ineligible)
    if present != wanted:
        why.append(f"incomplete gated set: missing or short {sorted(wanted - present)}")
    summary["gate_eligible"] = not why
    summary["prior_eligible_runs"], summary["prior_runs"] = prior
    summary["evidence"] = summary["gate_eligible"] and not prior[0]
    if not summary["evidence"]:
        summary["evidence_reason"] = (
            "not gate-eligible" if why else
            f"an earlier gate-eligible run exists: {prior[0]}")
    if why:
        summary["gate"] = "not gate-eligible"
        summary["gate_ineligible_because"] = why
        for e in summary["cases"].values():
            if "budget" in e:
                e["budget"]["result"] = "not gate-eligible"
    else:
        passes = [summary["cases"][n]["budget"]["result"] == "pass" for n in wanted]
        summary["gate"] = "pass" if all(passes) else "fail"
    (out / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("gate:", summary["gate"], summary.get("gate_ineligible_because", ""))
    for name, e in summary["cases"].items():
        verdict = (e.get("budget") or {}).get("result", "recorded")
        print(f"{name:48} p50 {e.get('p50_ms')} p95 {e.get('p95_ms')} p99 {e.get('p99_ms')} "
              f"max {e.get('max_ms')} fail {e.get('failures')} {verdict}")


if __name__ == "__main__":
    main()

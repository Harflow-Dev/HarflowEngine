# Store read-path measurement protocol (E1 slice S2)

Status: pinned before the first measurement run of slice S2. Everything below
is fixed; results are interpreted against it and nothing here is retuned after
measuring. Any later change is made as a dated amendment at the end of this file
that states what changed and why, and results gathered under the old text stay
labelled with it. The budgets come from the S03-ratified list retained in
[R01 validation](decisions/R01-validation.md) (measurement and evidence
protocol). The design that uses these measurements is
[store read path](store-read-path.md).

## Scope and honest limits

- Public read operations do not exist yet. S2 therefore measures the cost of
  opening a store plus the existing internal queries (stored status, task page,
  event page). The full protocol applies at S6, once every read exists.
- `version-info` does not touch a store. Its budget is covered here only by the
  null-process baseline (process start with no store); the real figure belongs
  to S6.
- The harness is `#[ignore]` integration tests under `crates/hf-storage/tests/`.
  It adds no release-binary surface and no dependency. It uses the storage test
  lease adapter (a non-blocking `flock` try-lock), not the production platform
  adapter, because `hf-storage` tests cannot depend on `hf-platform`. The
  production adapter is also non-blocking (a held lock returns busy at once,
  per its source), so busy outcomes behave alike; its `fcntl` byte-range
  mechanics and process-wide lease table are not exercised.
- Candidate read paths cannot run production queries (the query code is bound to
  the read-write `Store` handle and S2 changes no production code). A candidate
  is measured as its open phase plus representative SQL of the same shape; its
  query phase is taken from the existing path. This substitution is stated in
  every table that uses it.

## Datasets and generator

Both datasets are produced by one generator with a fixed seed
(`0x48465332`, a 64-bit xorshift stream; no wall clock, no OS randomness, apart
from the store's own `randomblob` generation id which is not used by any query).
The same seed and parameters give byte-identical logical content.

| name | tasks | events per task | events | notes |
| --- | --- | --- | --- | --- |
| `ci` | 300 | 100 | 30,000 | the R01 CI dataset, minus workflows |
| `large` | 5,000 | 100 | 500,000 | the R01 representative task/event counts, minus workflows and runs |

Deviation, recorded: the R01 CI dataset has 5 workflows (and the representative
dataset 50 workflows and 500 runs). No workflow or run storage exists in
schema 5, so those rows are omitted. The task and event counts are as specified.
The stress dataset (50,000 tasks, 5,000,000 events, 20 fake owners) is not run
in S2.

Content, all derived from the seed:

- Task ids `t-<8 hex>`; stamps strictly descending with about 2 percent ties;
  20 sessions; `active` true for about 30 percent, false for about 60 percent,
  unknown for the rest; header 160 to 480 bytes; status 64 to 256 bytes.
- Event records: 96 percent valid (kind drawn from the ten engine kinds, body is
  JSON of 170 to 1,200 bytes, `seq` from 1), 2 percent blank, 2 percent
  malformed; contiguous byte offsets; stream facts `available`, capture live.
- Written through the existing fixture APIs only: `Store::initialize_fixture`,
  `replace_task_fixture`, `replace_event_fixture` (one transaction per task),
  `checkpoint_passive`. No raw SQL writes, except the one explicit TRUNCATE
  checkpoint used to derive the checkpointed state (below), because
  `Store::recover` cannot qualify a store above the 64 MiB cap that this work
  exists to replace.

Store states per dataset (derived from one generation, copied with `cp -c` /
file copy, never regenerated between states):

| state | content | `-wal` | `-shm` |
| --- | --- | --- | --- |
| `wal` | all data committed through the store, no checkpoint ever run (checkpointing is off by construction) | holds everything | left by the writer's last close |
| `ckpt` | the same data after `PRAGMA wal_checkpoint(TRUNCATE)` by a raw SQLite connection | empty | left by that connection |

The `wal` state is what an E1 store looks like. `ckpt` is what a
checkpoint owner would maintain. Variants for experiments (missing `-shm`,
read-only directory, corrupted pages) are derived from these by copying.

## Query mix

One sample is one cold or warm operation performing: open (including the
qualification that path performs), then one query. The query rotates
deterministically through three kinds, so each kind gets one third of samples:

| kind | existing query | argument draw |
| --- | --- | --- |
| status | `TaskQuery::StoredStatus(id)` through `Store::task_read`, global scope | uniform over tasks (seeded) |
| list | `TaskQuery::page(50, unfiltered, no cursor)` | first page |
| events | `EventQuery`, limit 50, default filter, newest first, no cursor | uniform over tasks (seeded) |

The event page budget is therefore exercised on a task chosen uniformly, whose
100 records fit within a few pages. Filtered list pages, cursors and the
`from_oldest` mode are not in the mix at S2 (S6 adds them).

## Definitions of cold and warm

What can be controlled without privileges is the process and the SQLite state,
not the operating system's file cache. Stated plainly:

- **Cold (process-cold)**: every sample runs in a freshly executed child process
  that has never touched this store: new address space, new connection, empty
  SQLite page cache, no prior lease, nothing cached in the engine. This is how
  every `harflow-ctl` invocation runs. The operating system's unified buffer
  cache is not controlled: the files were written minutes earlier and the
  cache is expected to be warm. `purge` and cache-dropping interfaces need
  privileges and are not used. The consequence: reported "cold" numbers are
  a lower bound for a machine whose store files are evicted. A first read after
  eviction is not measured and is listed as unmeasured in the design draft.
  Where an experiment can honestly improve on this (for example, a store file
  written fresh by a copy) it is labelled, not relabelled as cold.
- **Warm**: the same process, the connection and any per-connection state
  already open, the same kind of query repeated; each sample runs only the
  query. 1,000 samples after 20 discarded warm-up samples (warm-up count is
  fixed here, discards are not applied to cold samples).

## Per-sample timings

Inside the child the monotonic clock is read at: entry of the measured function,
after each phase, and at result materialisation. Phases for the existing path:
`open` (the whole `Store::open`), `query`. A separate component test replays
the steps of `Store::open` against the same files (qualification copy, header
check, `quick_check(1)`, read-write open, identity and pragma round trips) with
the step boundaries timed; its sum is compared with `open` and the gap reported.

The parent measures spawn to exit as wall time. A null child (the same test
binary entry point that does nothing) is sampled with the same count and gives
the process start overhead. **Handshake overhead is reported separately**:
end-to-end wall time minus the child's own operation time, and the null-child
distribution, are given next to, never folded into, operation time.

Budget metric, fixed: the child's operation time (open plus query) against the
budgets in the next section. End-to-end wall time (including the null-child
overhead) is also reported and the design draft states whether the budget
would hold with it included.

## Budgets (not retuned)

On a release build:

| operation | p95 limit |
| --- | --- |
| cold version-info (no store) | below 50 ms |
| cold status | below 50 ms |
| cold list page | below 100 ms |
| cold event page | below 100 ms |

Write transaction p99, idle RSS and launch acceptance are outside S2.
Budget gating applies on Apple Silicon (this machine); numbers from other
machines are recorded, not gated.

## Sample counts and statistics

| case | samples |
| --- | --- |
| cold process, per dataset and state and path | 200 (about 67 per query kind) |
| warm, per dataset and state | 1,000 |
| null child | 200 |
| contended cold | 200 readers (4 concurrent reader processes, 50 each) |
| components | 30 replays per dataset |
| killed-reader and corruption experiments | one run each, outcomes recorded exactly, repeated 5 times where timing-dependent |

Percentiles are nearest-rank on the sorted samples (p95 of 200 is the 190th
value) with p50, p95, p99 and max. No outlier is dropped. Failed samples are
kept in the latency distribution with the time at which they failed, and
reported as a failure rate per case with the error kind. A refusal (for example
a busy lease or a size-limit refusal) is a recorded failed request, never
excluded from latency or budget accounting.

## WAL-grown and contended cases

- **WAL-grown**: the `wal` state of both datasets (CI roughly 15 to 30 MiB of
  WAL, large several hundred MiB) with nothing checkpointed. Cases on the
  existing path above the 64 MiB cap are expected to refuse; the refusal is
  recorded as the result, and the cost the removed copy would have had is taken
  from the component test.
- **Contended**: a separate writer process, holding a store opened by the existing
  writer path, commits batches of 8 mutations of 1 KiB each through
  `apply_batch` in a loop (no sleep, one transaction each, `synchronous=FULL`)
  on a copy of the dataset while 4 reader
  processes take cold samples sequentially. Writer commits per second are
  recorded. Reader failures (busy, deadline) are counted and kept in the latency
  distribution. A second contention variant adds periodic passive checkpoints by
  the writer.
- Deadline given to every operation: 5 seconds, so that busy results are
  distinguished from slow ones.

## Hardware, OS and toolchain, recorded per run

Each run directory contains `env.txt` with: date and time (UTC), git head and
`git status --short` summary, `rustc -Vv`, `cargo -V`, SQLite version string
reported by the linked library, `rusqlite` version, build profile and flags,
`uname -a`, `sw_vers`, CPU model and core count, RAM, the filesystem and mount
of the store directory, free space, the exact commands used, the load average
(1, 5, 15 minute) at the start and end of every batch, and the process count of
other `cargo`/`rustc` jobs visible at those moments. Other sessions share this
machine. A batch whose 1-minute load average at start or end exceeds 5.5 (half
of the 11 cores) is flagged loaded. A flagged batch is repeated (up to three
attempts); a result is relied on only if it comes from an unflagged repetition
or repeats a flagged one within the sample spread. If no unflagged repetition
is obtained the result is reported as load-compromised and not used to claim a
budget is met. All attempts are retained.

## Experiments that are not latency measurements

The design draft depends on measured answers to these, each recorded with exact
commands, file listings before and after, and outcome:

1. `SQLITE_OPEN_READ_ONLY` against a WAL store: (a) `-wal` and `-shm` both
   present; (b) nonempty `-wal`, no `-shm`, writable directory; (c) the same in
   a read-only directory. For each: sidecars created, whether WAL contents are
   visible (row counts against the known logical content), or failure, or silent
   omission. Negative control: `immutable=1`.
2. Reader latency under a concurrent writer (contended case above).
3. A reader killed at several points: remnants on disk under the current path
   and under the candidate path, and the next open's behaviour.
4. Corruption encountered during a read: a damaged main-file page (table leaf,
   index page, interior page, header), a truncated main file, a damaged WAL
   frame (middle and tail), a truncated WAL; whether the result is a typed
   error, empty or partial results, or wrong rows; and what a cheap structural
   check can detect.
5. Safety of a shared `store.lock` lease with no `io.lock` against the single
   writer, per the lock hierarchy in [storage protocol](storage-protocol.md):
   snapshot consistency of reads under writes and checkpoints, refusal of
   `recover` while readers hold the lease.

## Raw data

Raw samples (one JSON line per sample), environment files, store listings and
experiment transcripts are retained under
`.claude-work/engine/e1/s2-measure/` outside the repository, one directory per
run. The repository holds only this protocol and the design draft with the
summarised numbers.

## Amendments

2026-11-07, all made after the first old-path results and before the candidate
results; no budget, sample count or statistic was changed.

1. A third store state, `tail`, was added: the `ckpt` store plus 200 committed
   transactions of 8 x 1 KiB `fixture_records` rows left in the WAL (about
   5.5 MiB), written with a raw SQLite connection because the existing writer
   cannot open a store above the size cap. It represents what a checkpoint owner
   maintains. The existing path was measured on it too.
2. A `holder` variant was added to the cold cases: another process keeps a
   read-only connection open, as a live owner would, so the wal-index is valid
   and no reader has to rebuild it.
3. The contention writer uses a raw read-write connection (no `io.lock`) when
   the existing writer path refuses the store (large stores). Writer failures
   are counted either way.
4. Read-only directory experiments run in a directory outside the measurement
   tree, because the tree has inherited ACLs that grant file creation despite
   read-only mode bits; the first attempt in the tree was discarded.
5. The `ckpt` state was first generated with a connection that deleted its
   sidecars on close; this was corrected (checkpoint-on-close disabled, as the
   store does) and both datasets regenerated before any measurement.
6. Repetitions: three (r1 to r3) of every case. Load average is recorded at the
   start and end of each batch (`batches.jsonl`). r1 had 11 of 31 batches above
   the 5.5 threshold, r2 had 3 (contended cases, at most 5.9), r3 had none
   (maximum 5.2). A fourth repetition was started and aborted on request while a
   timing-sensitive run was in progress; its partial data is kept as invalid.
   Results are quoted from unflagged repetitions, with flagged ones shown
   beside them.
7. "Candidate" is defined as: shared `store.lock` lease, no `io.lock`, marker
   and header checks, `SQLITE_OPEN_READ_ONLY`, checkpoint-on-close disabled,
   identity query; queries are representative SQL as stated in the scope
   section.
8. Harness files: `crates/hf-storage/tests/read_measure.rs` (+ `read_measure/`)
   for datasets and timing, `read_experiments.rs` (+ `read_experiments/`) for the
   non-latency experiments. Run by `cargo test --release -p hf-storage --test
   <name> -- --ignored --exact <test>` with `HF_MEASURE_DIR`, `HF_MEASURE_OUT`.

2026-11-07, second revision after review. Edits only; nothing below has been run
yet, and results gathered before it keep the criteria they were gathered under.

9. Remnant capture. Killed-process trials now record directory contents
   immediately after the reap, before the verification read (which can itself
   create `-shm`). Confirmed-stage kills (after a handshake line from the child:
   `BEGIN` printed before the open starts, `READY` after it completes) are
   separate from spawn-relative timing trials, which are labelled as such and
   are not described as kills during recovery.
10. Correctness comparison, scope as built. The raw-SQL corruption probe hashes
    every column of the newest 50 event records per task and compares them with
    event-record vectors regenerated from the generator (not another store read);
    the pristine store is checked against that baseline in every run. The
    late-corruption trials compare the production `event_read` record vectors with
    the pristine result and check each returned record for membership in the
    generated set (membership only, not equality with an expected page). The
    status and list views are compared only against the pristine result, as a
    debug string. Only event records are compared against an independent
    baseline. The
    candidate SQL now decodes every column of every row (it decoded only column 0).
11. Contention. Writer failures are reported as errors over attempts (not over
    successes), separately for the window in which readers run (a marker file
    marks it), with the reader window length recorded. Readers per run are
    configurable (`HF_MEASURE_CN`, 250 per reader for reruns) so the overlap is
    long enough to support a claim. A no-writer variant (`HF_MEASURE_NOWRITER=1`)
    measures 4 concurrent cold readers with no owner and no holder.
12. Per-kind p95 (status, list, events) is reported for every case so a budget
    miss is attributable to a kind.
13. New experiments: `-shm` and `-wal` retention after each kind of clean close
    and after `recover` (`shm_retention`), and, with the production lease
    adapter, `recover` against readers in both orders and concurrently
    (`crates/hf-host/tests/store_recover_lease.rs`, macOS arm64 only).

2026-11-07, reruns of the items amended above (corrected harness, build hold
lifted). Corruption, late corruption, killed readers and `shm_retention` ran
once at load 3.9 to 4.2; the production-lease recover tests ran with
`--ignored` (two tests there failed when run in parallel because the platform
lease try-locks one process-wide mutex, so they now serialize); contention ran
as three repetitions c1 to c3 (labels `c1`, `c2`, `c3`; 12 batches each, all
at or below load 5.09) with 1,000 samples per reader for the writer cases and
250 per reader for the no-writer cases. The first corruption, late-corruption
and killed-reader files are kept under `raw/experiments/first-run/` and the
first contention data under the `r1` to `r3` labels; their weaknesses are
described in amendments 9 to 11. The file-size limit forced splitting the
harness (`late.rs`, `retention.rs`); no behaviour changed.

2026-11-08, slice S6 (E1 exit evidence). Written before any non-smoke S6 sample. Smoke runs
(labelled `smoke`, 5 samples per case, gated cases on `ci` and the ungated components) were
taken before this text was committed to check the harness; they are not evidence. The harness
(`scripts/measure-ctl-budgets.py`) and this section are committed before the first gated
run, so the criteria can be checked against the commit. The S2 text above and the S2 results
keep their criteria. No budget value is changed. S6 differs from the protocol only as listed.

S6.1. Gating boundary. Gated cases are the `ci` dataset only, in the `wal`, `ckpt` and
`tail` states (amendment 1 defines `tail`), measured on the release `harflow-ctl` as cold
processes on this Apple Silicon machine. Source: the completion plan's exit gate, "Reads
meet revision 3's budgets on the CI dataset" (`HARFLOW_ENGINE_COMPLETION_PLAN.md` lines
117-118, workspace root), with the protocol's Apple Silicon gating (budgets section). The
budgets are the table above, unchanged: version-info below 50 ms, status below 50 ms, list
page below 100 ms, event page below 100 ms, each at p95. The `large` dataset is recorded,
not gated (S6.8).

S6.2. Metric. The gated figure is the end-to-end wall time of one `harflow-ctl` process,
from spawn to exit, measured by the parent. The binary has no phase timer, and end to end
is stricter than the protocol's operation time. The null child (`/usr/bin/true`, 200
samples) is reported beside it and is never subtracted for the gate. A case that misses end
to end but would pass net of the null child is reported as a miss. A case is a pass only
when its p95 is below the limit, it had no failed sample, and its batch is not
load-compromised. The harness gives a gated verdict only for a run that has the complete
gated set (four operations in `wal`, `ckpt` and `tail` on `ci`), each at exactly 200
samples, a label other than `smoke`, and no `--only` filtering; any other run is marked
"not gate-eligible" in its summary and carries no pass or fail. Runs under one output parent are serialized by an exclusive lock file
(`.measure.lock`, holding the pid and run name); a second run refuses to start and a stale
lock is removed by hand, never automatically, so a run directory without a summary is a
dead run. The evidence is the first
complete non-smoke run at the default sample count with no `--only` filtering and no
requested recorded group missing its data; every non-smoke run label is reported in the
receipt. The summary records `samples`, `only`, `missing_data`, `gate_eligible`, `evidence:
true|false` (true only for a gate-eligible run with no earlier gate-eligible sibling run), the
earlier sibling runs (`prior_eligible_runs`, and `prior_runs` for the others, a killed run
without a summary included),
and a gated case whose sample count differs from 200 fails with the reason `n != 200`.

S6.3. Samples. 200 cold samples per operation per state. The protocol had 200 per path
across three query kinds (about 67 per kind), so this is more samples per kind, never
fewer. Percentiles are nearest-rank (p50, p95, p99, max); no outlier is dropped; failed
samples and refusals stay in the distribution and are counted by error kind. The task id of
a status or event sample is drawn uniformly from the dataset, as a pure function of the
dataset name and sample index.

S6.4. Warm cases are dropped. A `harflow-ctl` invocation is always a fresh process, so a
warm case has no counterpart in the shipped surface; warm was a library-level diagnostic of
S2. The S2 warm results stand and are not repeated.

S6.5. Holder variant (amendment 2): a second process keeps a read-only connection open
while the four operations are sampled, for each state. Recorded, not gated. The holder is
the existing ignored harness entry (`child` with `HF_MEASURE_OP=hold`).

S6.6. Query additions (listed as S6's in the query mix section): `task-list-page` with a
`--session-id` filter, a list cursor page, an event cursor page, and `task-events --from
oldest`. Cursors are fetched once, outside the timed region, from a fixed pool of 20 tasks
(list cursor: one cursor from the first page). Recorded for `ci` in the three states and
judged against the parent budget (list page or event page); they are not gated.

S6.7. Contention. The protocol's writer is kept: the existing ignored harness writer
(`child` with `HF_MEASURE_OP=writer`: `apply_batch`, 8 mutations of 1 KiB per transaction,
`synchronous=FULL`, no sleep), on a copy of `ci` `wal`, with and without passive
checkpoints every 200 commits. Four concurrent cold `harflow-ctl` readers take 50 samples
each, rotating status, list page and event page. Recorded, not gated. Reported: writer
commits per second (whole run and inside the reader window), writer errors, and reader
`state_busy` counts; busy results stay in the latency distribution. The reader window is
marked by a file as before. A contention case counts as evidence only if the writer exited
with status 0, printed exactly one report, ran in the existing `store` mode, committed inside
the reader window (and, in the checkpoint variant, ran at least one checkpoint); otherwise
the case is marked invalid in the summary with the writer's output kept, and is not
contention evidence.

S6.8. Large dataset (5,000 tasks, 500,000 events): the four operations on `wal` and `ckpt`,
recorded and not gated, as S6.1 gates `ci` only. The S2 `wal` miss on `large` (fixture-only
WAL growth with no owner at E1; the checkpoint owner is E2) is not re-litigated here.

S6.9. Components, recorded with no budget: `/bin/ps -axo pid=,ppid=` invoked as the caller
gate invokes it (cleared environment, `LC_ALL=C`, 200 samples); release binary size and
linked libraries (`otool -L`); `task-prompt` on a task with a 4 MiB prompt (stored as a
digest-verified sidecar, the form long prompts take; an inline 4 MiB prompt is also
sampled and is expected to be refused by the reader's byte bound), against `task-prompt`
on a short prompt; `status-batch` of 20 ids, `list`, `task-artifacts`, `interaction-events
--limit 100` and `recovery-activity`, on `ci` `wal`; and `statistics-page --limit 100` on
`ci` `wal` and on a 10,000-task world built by the same generator (10 events per task),
measured on its `ckpt` state because the generator's `wal` state of that world is about
1.2 GiB and is not a state any store takes in E1.

S6.10. Generator. Task rows become valid documents, because the S2 content (random
alphabetic header and status bytes) cannot be served by public reads. Header and status
blobs are JSON objects padded with a `pad` member to the protocol's byte ranges (header 160
to 480, status 64 to 256). The seeded size draw, the `active` mix, the 2 percent stamp ties
and the event content are unchanged: the valid generator consumes exactly the S2 random
draws. Stamps are read as seconds and shifted 600,000,000 s into the past, so no task
starts after the clock. Status follows the `active` draw (`running` without an exit code;
`completed`, exit 0; and, for the draw that was unknown, `failed`, exit 1, since
`add_task_fixture` derives `active` from the status and cannot express unknown). Typed
facts (backend, session, start and finish times, exit code, drained flag) agree with the
documents, and each task has a short inline prompt. Written through the existing fixture
APIs only: an empty `replace_task_fixture` for coverage, then `add_task_fixture` per task;
events as before; `tail` by the raw derivation of amendment 1. The S2 content remains
available with `HF_MEASURE_TASKBYTES=legacy` (and is the only content the S2 results
describe). The generated `ci` `wal` state holds about 66 MB of WAL (65,882,952 bytes, about 62.8 MiB, in the
smoke generation), a deviation from the protocol's 15 to 30 MiB for `ci`, caused by the
per-task `add_task_fixture` writes, just below the 64 MiB (67,108,864 bytes) cap that governs the old open path.

S6.11. Readability check. Before any sample, the harness runs every gated and recorded
command family on the generated `ci` store in all three states and refuses to sample if one
fails: `version-info`, `state-inspect`, `list` (300 tasks, none refused), `status`,
`status-batch`, `task-list-page` (plain, filtered, cursor), `task-events` (default, cursor,
`--from oldest`), `task-prompt`, `task-artifacts`, `recovery-activity`,
`interaction-events` and `statistics-page`. Each must answer with exit 0, the expected
format and a non-empty result where one is expected.

S6.11a. Deadline. The protocol gives every operation a 5 second deadline. The harness
enforces only a 30 second per-process timeout (a timed-out sample is a failure at that
time); `harflow-ctl` applies its own internal deadline, which the harness does not set.

S6.12. Load rules are the protocol's (1-minute load average at most 5.5 at batch start and
end, up to three attempts, all attempts kept), with a 10 second pause between attempts.
A load-compromised result never counts as meeting a budget. Raw samples (one JSON line per
sample per attempt), `env.txt` (including the SQLite version string the linked library
reports, printed by the `read_measure` helper), `batches.jsonl` and `summary.json` go to a
new run directory `<UTC timestamp>-<label>` under a parent outside the repository. The
directory is created exclusively, so a run never overwrites or truncates earlier evidence,
and each run's `batches.jsonl` names only its own raw files. A run labelled `smoke` is a
harness check and not evidence.

## Write-path measurement runbook (E2-A slice A7b)

The commit-watermark write measurements live in `crates/hf-host/tests/write_latency.rs` (+
`write_latency/`). Every test is `#[ignore]` and compiled only on macOS arm64. Results and the
owner decisions are recorded with the A7b slice; this section only says how to rerun them.

Dataset. Generate the `ci` dataset with the read generator above; the write tests clone its
`ci-ckpt` root (checkpointed, empty `-wal`) with `cp -cR` for every series and remove each clone
when the run ends, also on failure:

    cargo test --release -p hf-storage --test read_measure --no-run
    HF_MEASURE_DIR=<data> HF_MEASURE_OUT=<out> HF_MEASURE_DATASET=ci \
      <target>/release/deps/read_measure-<hash> --ignored --exact --test-threads=1 generate

Run. Build with `cargo test --release -p hf-host --test write_latency --no-run`, then:

    env HF_MEASURE_DIR=<data> HF_MEASURE_OUT=<out> HF_MEASURE_LABEL=<label> [HF_WL_...] \
      <target>/release/deps/write_latency-<hash> --ignored --exact --nocapture --test-threads 1 <test>

| test | what it measures | settings |
| --- | --- | --- |
| `latency` | gate: one fresh writer child per operation, interleaved series | `HF_WL_SERIES` (`shape/mode/policy`, comma separated; shape `create` or `create_append`, mode `fsync`, `full` or `default`, policy `suppressed` or `owner`); `HF_WL_RESET` (default 50, the gate definition: re-clone every root after each 50 rounds to bound `-wal`; 0 lets it grow); `HF_MEASURE_WARM` (50), `HF_MEASURE_N` (500) |
| `in_process` | per-commit cost on one long-lived Suppressed handle per mode | `HF_MEASURE_WARM`, `HF_MEASURE_N` |
| `contention` | four writer processes, 200 KB appends | `HF_WL_MODE` (`full`), `HF_WL_POLICIES` (`owner,suppressed`), `HF_WL_COMMITS` (100) |
| `throughput` | four tasks on one root replaying burst profiles, then 4 x 42 lines/s sustained | `HF_WL_MODES` (`full,fsync`), `HF_WL_PROFILES` (default: the checked-in r06 and r07), `HF_WL_REPEAT` (3), `HF_WL_SUSTAINED_S` (60), `HF_WL_CAPTURE` (1; 0 drops the capture-publication commit) |
| `max_rate` | one writer, one 1 KiB line per commit, 30 s | `HF_WL_MODES` |
| `full_flush_check` | `F_FULLFSYNC` on a regular file, `/dev/null` and a pipe | none |

The gate is `create/full/suppressed` (or `create/default/suppressed`, which follows
`DEFAULT_DURABILITY`) with the 1-minute load average at most 4.0 at start and end; `batches.jsonl`
records it. Raw output is one JSON line per sample in `HF_MEASURE_OUT`.

Profiles. `write_latency/profiles/r06.txt` and `r07.txt` hold one `t_ms len` pair per stdout line
of the scrubbed E2 recordings `r06_tool_use` and `r07_permission_denied` (time from spawn and
line length; no content). They are produced from each recording's `timeline.jsonl` by:

    python3 -I -c 'import json,sys
    for l in open(sys.argv[1]):
        r=json.loads(l); print(r["t_ms"], len(r["line"].encode()))' <recording>/timeline.jsonl

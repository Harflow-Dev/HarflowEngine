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

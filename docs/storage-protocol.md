# Cooperative storage protocol, R02-A2

Cooperative store protocol used by internal fixtures and public read-only ctl
adapters; no public execution qualification. See
[foundation](storage-foundation.md) and [receipt](migration-receipts/R02-A2.md).
The R02-A receipt remains historical evidence for its exclusive/quiescent design.

## Authority and identity

New fixtures publish a bounded immutable protocol/schema/generation marker after
SQLite initialization and the initial checkpoint succeed. Existing R02-A fixtures
without the new marker/lock inventory refuse; there is no silent upgrade, import,
reset or adoption. Root, database, marker and lock bindings are checked, as are
regular-file sidecars. Marker identity must match the SQLite logical identity.
The marker is bootstrap ownership/compatibility evidence, not another mutable
record authority or permission to replay an external effect.

Roots are local same-host cooperative Unix store directories. All participating
writers must use this protocol. Raw SQLite schema writers that bypass gates and
hostile same-user path replacement are outside the guarantee. Observed replacement
or symlinks refuse; metadata checks are not an atomic hostile nofollow boundary.
No network filesystem, Linux/Windows runtime, reboot or power-loss qualification
follows from macOS process tests.

## Lock hierarchy and lifetimes

1. Lifecycle `store.lock`: shared for normal Store lifetimes and probes; exclusive
   for initialization and explicit recovery/TRUNCATE. All actions acquire this
   before the I/O gate. No lock upgrade is assumed atomic.
2. Per-handle mutex: normal operations reject overlap rather than queue work.
3. `io.lock`: exclusive while copying qualification snapshots, opening source
   SQLite, applying writes, or running checkpoints/recovery. It serializes main/WAL
   changes across processes, not the lifetime of every normal handle. Read
   sessions do not take it. Handle
   methods acquire mutex before I/O; there is no reverse acquisition path.
4. SQLite's own writer/read/WAL locks provide transactional isolation. Read batches
   use SQLite snapshots; a long read may limit checkpoint progress.

Admission is bounded and does not create work after a failed/expired request.
Busy/deadline results do not assert anything about another owner's effects.
A process's death releases its kernel-held locks, not future engine execution,
provider effects, resource reservations or unknown mutation outcomes.

## Nonmutating qualification and explicit source opening

Open, recover and probe qualify the existing store in place; there is no copy, no
scratch directory and no size cap. Before SQLite is opened they check, in this order,
the marker, the database header, that the rollback journal is empty, and that `-wal`
exists and is a regular file (`lstat`), then the lost-WAL acceptance record (below).
A missing `-wal` is refused (`WalMissing`) and none is created, because SQLite would
create an empty one and report an empty history.
They then open the source with `SQLITE_OPEN_READ_ONLY` and checkpoints suppressed,
compare the logical application/schema/generation with the marker, and re-check the
`-wal` type and identity and the marker after the open. Qualification failure never
authorizes source recovery. No custom WAL parser, immutable live-WAL shortcut, sidecar
deletion or original-file reset is used.

- `open` checks marker, header, journal, `-wal` and identity only. It runs no
  `quick_check` (owner decision A2).
- `recover` runs one `quick_check` through the read-only handle under exclusive
  `store.lock` and `io.lock`, then requests the TRUNCATE checkpoint.
- `probe` runs one `quick_check` through the read-only handle under shared
  `store.lock`, without `io.lock`: a read-only snapshot is consistent and a probe
  never delays a commit. Its deadline is a parameter (`probe_with_deadline`; `probe`
  uses one second).

Only the scan is deadline-bounded. The read-only open, WAL-index rebuild and identity
reads are bounded by `busy_timeout` and an after-the-fact deadline check.
The `quick_check` is bounded by the caller's deadline: a watchdog thread interrupts
the statement when the deadline passes and the operation fails with
`DeadlineExceeded`. The watchdog re-issues the interrupt every 20 ms after the deadline, because SQLite clears the flag when a statement starts. It is joined before the call returns, and the scan
connection is dropped, never reused.

“No source writes” means no main or WAL content change, no `-wal` creation or
removal and no sidecar deletion by probe. Like a read session, a probe may create or
rewrite `-shm`; ordinary file reads may affect access metadata. A missing root returns
absent before any lock creation. Foreign, unknown, future, corrupt and mismatched
identities refuse. Only `RecoveryReadSession` still reads a bounded private copy (see
[recovery observation](recovery-observation.md)); it keeps its 64MiB cap and scratch
parent.

If an external actor deletes `-wal` between the precheck and the open, SQLite's
read-only open can create an empty one; the later `-wal` identity re-check reports
`Replaced` (race acknowledged, as for the read session). `open` and `recover`
repeat that re-check after their source open.

Error mapping changes from the old copy path: a store over 64MiB now qualifies instead
of failing with `LimitExceeded`; a missing or non-regular `-wal` is `WalMissing`
(a symlinked `-wal` is `WalMissing`, not `InvalidRoot`); corruption found while
opening or scanning is `Incompatible`; probe proceeds while `io.lock` is held.

Normal `open` is explicitly an internally authorized mutating operation: after
qualification under the I/O gate, it may let SQLite recreate SHM/WAL-index
and recover legitimate committed WAL. It never converts a probe into recovery.
Explicit `recover` additionally requires exclusive lifecycle ownership and requests
SQLite TRUNCATE checkpoint. Live Store handles block that maintenance rather than
being forcibly closed. Nonempty rollback journals refuse; this is a WAL-only recovery primitive. No
schema migration operation is added; future schema-change
work must use exclusive lifecycle, preserve generation/provenance and be separately
assigned. Unknown outcomes do not permit reset/replay.

## Lost-WAL acceptance record

`store.wal-acceptance` in the state root holds
`harflow-wal-acceptance-1\n<generation>\n<pending|accepted|failed>\n`, at most 128
bytes, read `lstat`-first as a regular file. Absent means no acceptance is in progress.
`accepted` for the marker's generation lets the root be used. `pending` and `failed`
are refused as `WalMissing` even when an empty `-wal` exists. A foreign generation,
malformed or oversized content, or a non-regular file is `Incompatible`. Open, recover,
probe, the read session and `state-inspect` check it immediately after `-wal` and
before any later check; readers never create or write it. Open, recover and probe check
it before the SQLite open and again at the end of in-place qualification, next to the
`-wal` identity and marker re-reads; the read session checks it at open, at every read
entry and at every read exit (`check_files`, with the session's generation). Only the
maintenance actor writes it (E2-B), in the order `pending`, qualify, `accepted` or `failed`.

Race acknowledged: the storage reader checks the file with `lstat` and then opens it
without `O_NOFOLLOW`/`O_NONBLOCK` (hf-storage has no libc dependency), as the marker read
does. A local writer that swaps the record for a FIFO or symlink between the two calls can
make a read wait or follow the link; the `fstat` after opening still requires a regular
file. `state-inspect` opens it with `O_NOFOLLOW|O_NONBLOCK` and is not affected.

## Read session

A read session (`ReadSession`) answers reads from the real store without a copy.
It takes `store.lock` shared (acquisition capped at 2s) and never `io.lock`, and
it writes no main or WAL content. Before SQLite is opened it checks the marker,
the database header, that the rollback journal is empty, and that `-wal` exists
and is a regular file (`lstat`), and the lost-WAL acceptance record; a missing `-wal`
is refused because opening would create an empty one and report an empty history. A `-wal` deleted between that
check and the open is not detected. It then opens with `SQLITE_OPEN_READ_ONLY` and
checkpoint-on-close disabled, compares the database identity with the marker, and
runs the existing queries in one read transaction. It runs no `quick_check` and has
no size limit of its own. The deadline is soft: SQLite open and WAL recovery cannot
be interrupted, so a read that overruns is refused after the fact as busy; the
caller supplies 5s for a read. Any SQLite error is a refusal, and `ReadRefusal`
classifies refusals for the transport above. Open, recover and probe qualify in
place (see above); only `RecoveryReadSession` keeps a capped private copy.

## Checkpoint and close policy

Source connections disable automatic checkpoints and checkpoints on close before
SQL access, using `wal_autocheckpoint=0` and the safe rusqlite wrapper for SQLite
`SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`. WAL/FULL/foreign keys remain checked. This avoids
an uncoordinated last-close checkpoint while another process copies source files.
Qualification opens the source read-only with the same suppression, so it never checkpoints and a
last close never removes `-wal`.

PASSIVE checkpoint runs behind handle admission and the I/O gate. It may coexist
with readers and must return frame counts and completeness truthfully: a zero busy
indicator is not proof that all frames were checkpointed. Exclusive recovery uses
TRUNCATE only after other lifecycle holders are gone; incomplete/busy results are
observable, never permission to delete WAL or force reader release. Disabling auto
checkpoint requires a scheduling/WAL-growth policy. The checkpoint owner below implements
one, but no production handle opts into it yet: every production opener uses `Suppressed`,
so production WAL growth stays unbounded until the slice that wires `Owner` into the owner
runtime (E2-D).

### Checkpoint owner

`WriterOptions { checkpoint, flush }` picks the policy per handle: `Store::open_with`
takes it explicitly and `Store::open` is `WriterOptions::suppressed()`. `Suppressed` is the
behaviour above (no checkpoint of any kind) and is what the fixture seed, the host fixture
modes, the owner runtime writer and the watch seam use, so seeded roots keep a nonempty
`-wal`. `Owner` keeps both suppressions (`wal_autocheckpoint=0`, no checkpoint on close)
and checkpoints only through explicit PASSIVE: `PRAGMA journal_size_limit` 16 MiB
(`JOURNAL_SIZE_LIMIT`) is set in `open_source`; one PASSIVE runs at open under `io.lock`
after qualification (a busy result is ignored); `Store::write` calls `maintain_wal` after
the commit's `io.lock` is released, before the write returns, and any maintenance result,
including an error, leaves the committed result unchanged. `maintain_wal` reads the live
log size with `PRAGMA wal_checkpoint(NOOP)`, which copies nothing and takes no checkpoint
lock, though its WAL-index header read can run WAL-index recovery under locks; a busy or
unknown result is a skip. It returns at once while the log is at most 8 MiB
(`PASSIVE_AFTER`); the `-wal` file length is not used, because after a log restart the
file keeps its length while the log refills from the start. Past the threshold it re-takes
`io.lock` and runs PASSIVE. Its budget is the earlier of the write's deadline and 100 ms
(`MAINTENANCE_BUDGET`); the budget bounds admission, `io.lock` and busy waits, not the
PASSIVE copy itself. `Busy` or an expired budget is a skip, retried after the next commit.
Callers outside `Store::write` may call `Store::maintain_wal(deadline)` directly. When the
PASSIVE completed and `-wal` is still over 16 MiB (a reader had pinned it), one
maintenance commit follows: it only advances `commit_sequence`, but it writes a frame, so
SQLite restarts the log and cuts the file to the limit at that write. The result reports
whether that commit landed (`shrink_commit_landed`), not the resulting file length.
TRUNCATE stays in `recover` only and RESTART is never used, so `-wal` always survives.
`FlushPort` (`hf-application::durability`) is carried in the options for the watermark
flush (A7); the default is `sync_all`.

SQLite references: [WAL behavior](https://www.sqlite.org/wal.html),
[checkpoint modes](https://www.sqlite.org/c3ref/wal_checkpoint_v2.html), and
[connection configuration](https://www.sqlite.org/c3ref/c_dbconfig_defensive.html).
The snapshot protocol depends on every source mutator honoring the I/O gate and
actual no-checkpoint-on-close behavior; tests verify those assumptions on the
pinned library. Process kill is not device/filesystem power-loss proof, and a test
at a pre/post-operation barrier is not an interruption inside SQLite's VFS.

## Test-only interruption boundaries

Normal builds contain no stage hook or test environment control. Unit-test builds
can stop at named before/after source-open and checkpoint boundaries so the parent
can kill and reap its exact owned process. These prove those boundary cases and
lock release/reopen behavior; they do not instrument or certify interruption inside
SQLite recovery/checkpoint syscalls or a VFS write. Tests use bounded IPC and exact
invocation scratch ownership; those infrastructure guarantees are distinct from
future engine process containment.

## R03-B1 fresh-schema evolution

R03-B1 introduced SQLite schema2 and an actual schema2 marker. Schema1
fixtures refuse without automatic upgrade. The new internal ownership namespace
has its own version/generation checks and is separate from task/event fixture DML;
see [ownership foundation](ownership-foundation.md). Lock hierarchy, WAL/FULL/FK,
qualification (in place since E2-A), nonmutating probe/source-open distinction and explicit recovery
remain unchanged. This is new-root initialization, not a schema-migration API.

## R02-W fresh-schema evolution

The R02-W stage introduced schema3, with watcher metadata version1/revision0
created in the exclusive initialization transaction. Current schema6 retains it;
older and future roots refuse. No upgrade, reset or reader initialization occurs.
Metadata publication and revision advancement share one SQLite commit.
Watch-source reads inspect at most two scalar
rows and never complete a publisher or repair state. Store generation qualification
checks exact UTF8 byte length before allocation, including embedded-NUL refusal.
See [task-list invalidation](task-list-watch.md). All prior lock, WAL/FULL/FK,
manual-checkpoint limits remain unchanged; open/recover/probe no longer have a size cap.

## Subsequent schemas

The separate [private capture/completion foundation](capture-completion.md)
introduced schema4; schema5 changed the locking protocol. Current fresh roots use
schema7. Schema6 added `task_facts`, `task_processes`, `task_prompts`, `artifact_manifests`
and `artifact_items` beside the existing task/event/watch tables; schema7 replaces the
`task_pending` id list with `launch_attempts` (pre-record launch receipts: one row per receipt
filename stem, with invalid names, refused receipts and the status columns the ordinary
reads need) and adds `chain_stops` (one row per chain root: a `marker` with its raw
attribution text, or a `fault`; no row is not stopped). `task_facts.chain_root` is stored
text and is qualified when read: absent, a task identity, or invalid text kept verbatim.
`artifact_items` also carries the evidence of an `observed_backend_plan` reference (`tool_use_id`,
and `plan_directory` with the decimal-text `plan_device` and `plan_inode`): `tool_use_id` is
present exactly for that method, and the directory columns are all present or all absent. Each
item also stores `encoded_len`, its bytes in the reference manifest document (see
[Artifact writer](#artifact-writer)). Schema7 has not been released, so these columns did not bump
the version; a local root initialized before `encoded_len` existed still reads, but artifact writes
to it fail inside the transaction and roll back. For the same reason the
`commit_sequence` table (singleton row, `value` integer, seeded with 0 when the root is
initialized) was added to schema7 without a version bump; a root initialized before it refuses a
writer open and recovery as incompatible, before any mutation. The value counts committed write
transactions, including commits that change no row; it is not a change detector.
Header/status blobs remain the served bytes. Schema6 and schema5 roots refuse as
`unsupported_layout`, without upgrade or repair; no checked-in database fixture or
internal root depends on an older schema. Open/recover/probe qualify in place with no size cap; public ctl
reads use `ReadSession`, including `recovery-activity`. The internal operation-scoped
`RecoveryReadSession` remains a distinct capped private-copy path.

## Commit path

Every write after initialization goes through one function, `Store::write`
(`crates/hf-storage/src/write_tx.rs`), in this order: admission, the shared `io.lock`, a
`BEGIN IMMEDIATE` transaction, the caller's action, the publication the caller asked for, the
`commit_sequence` increment, the binding check, the commit and a binding check after it. A commit
failure is `CommitOutcomeUnknown`, and so is a failed post-commit binding check. The publication is
one of four variants: dataset generation rotation with a watch revision bump, a revision bump only,
a dataset (paging) generation rotation only, or nothing. `commit_sequence.value` therefore
increases by exactly one per committed write and never on a rollback, a pre-commit failure or a
read. Test-only stage hooks keep their names (`before_task_catalog_commit`, `ownership_before_commit`,
`conversation_before_commit`, `capture_before_commit`, `capture_before_terminal` and the matching
`after_` hooks) and are selected by a label passed to `Store::write`. Checks that must precede the
action (table presence, ownership or capture schema) run inside the action, in the same
transaction. Initialization writes the first schema in its own exclusive transaction and is the only
commit outside `Store::write`.

## Task writer

`Store::create_task` (`crates/hf-storage/src/task_writer.rs`) stores a new task in one commit:
the `task_facts` row, the `task_headers` row with the served header and status blobs, the
`task_prompts` row and an optional `task_processes` row; the commit rotates the dataset generation
and bumps the watch revision. An existing id, valid or damaged, is refused (`TaskExists`) with
nothing written. `Store::republish_header` replaces a valid task's served blobs and observation
columns in one commit with the same publication; when the stored recorded status is terminal
(`completed`, `failed`, `timed_out`, `cancelled`) and the new one is not, it returns
`KeptTerminal` and rolls back, so nothing is published and `commit_sequence` does not advance. The
fixture `add_task_fixture` goes through the same insert, with its prompt row stored as given.

| Bound | Value | Source |
| --- | --- | --- |
| Header blob, status blob | 2048 bytes each | header: reference `bound_header`; status: Rust bound |
| Inline prompt | at most 2000 characters (Unicode scalar values) | reference `PROMPT_PREVIEW_CHARS` |
| Sidecar prompt | above 2000 characters: preview of the first 2000 characters, cut on a character boundary; `source_name` `{id}.prompt.{sha256}.txt`; `claimed_sha256` and the blob set | reference prompt sidecar |
| Prompt text | at most 4 MiB (`PROMPT_VERIFY_BYTES`), so every stored sidecar is verifiable on read | Rust bound; the reference has none |
| `backend`, `model`, `started_at`, `finished_at`, `chain_root` | 2048 bytes each | Rust bound |
| `repo_path` | 4096 bytes | Rust bound (Linux `PATH_MAX`) |
| `title` | 65,536 bytes; with a 2000-character preview (at most 8000 bytes) and a source name it stays within the default 1 MiB read budget of the prompt read (checked at compile time) | Rust bound |
| `session_id`, `status` | 1024 bytes and 256 characters; 256 bytes | shared with the fixture writer |
| Process | `pid` 2..=`i32::MAX`; an identity's `pid` equals the row's; start seconds fit a SQLite integer | Rust bound |
| `republish_header` | header and status blobs 2048 bytes each, `recorded_status` 256 bytes, checked before the task is looked up | Rust bound |

All bounds refuse with `LimitExceeded` before any write. The fixture wrapper applies the blob,
session, status, pid and start bounds but not the fact text or identity-pid bounds, so fixtures
can seed over-long facts to exercise the read bounds.

The header JSON compaction that keeps a header within 2048 bytes belongs to the interface layer;
the writer takes the header bytes as given and refuses a larger one (`LimitExceeded`).

E2-D callers of the writer own what the reference store does around a record write and the
writer does not: carrying `interruption`, `total_cost_usd` and `observed_model` forward from the
existing record when the new one observed none (cumulative merge into the header and status
bytes); routing a task that does not exist yet to `create_task` (`republish_header` refuses it
with `TaskMissing`); and keeping `task_facts` current, since `republish_header` does not update
the facts row (status, exit code, finish time, drained flag).

## Event writer

`Store::append_events` (`crates/hf-storage/src/event_writer.rs`; `append_event` for one line)
appends terminated event lines to a valid task's log in one commit, in order, starting at the
stream's `complete_end`. Each `EventAppend` carries the raw line length without its newline
(`raw_len`), the `seq` and `kind`, and the stored `body`: the projection the interface layer's
`prepare_line` produces, which the reads serve. A record spans `raw_len + 1` bytes. A line over
1,000,000 bytes is stored as an `Oversized` record with no seq, kind or body (the caller's values
are neither checked nor stored), as the reference skips such lines unparsed. The body is not tied
to `raw_len`: a projection can be shorter or longer than its raw line. The writer stores only
`Valid` and `Oversized` records, the engine's own events; it cannot ingest a foreign log with
blank or malformed lines.

The commit publishes nothing to the task list: the dataset generation and watch revision stay,
and so does the stream generation, so event cursors survive an append. When the task has no
stream yet, the append creates one with a fresh generation and the caller's `facts_if_new`
(availability, capture state, drained flag, EOF seq, grace, task status); an existing stream keeps
its facts, which belong to the capture and terminal writers, and only its `complete_end` moves.
`facts_if_new` is checked only when it is used. An empty batch writes and commits nothing.

The append is checked against the stream's end only: the last record (one indexed lookup) must
end at `complete_end`, the stream must have no partial tail, and each valid event's seq must be
above the seq of the last valid record and of the batch's earlier events. The last valid record is
found backwards along `event_starts`: one lookup when the log ends in a valid event, otherwise a
walk back over the trailing oversized or malformed records, at worst the whole task's records
when none is valid. The log is not recounted and the store's fixture `Limits` do not cap
it.

| Bound | Value | Source |
| --- | --- | --- |
| Stored event body | 16,384 bytes, UTF-8 | reference `EVENT_MAX_BYTES`, after projection |
| Raw event line | 1,000,000 bytes without the newline (span 1,000,001); longer is `Oversized` | reference `MAX_LINE_BYTES` |
| Kind | 128 bytes | shared with the fixture writer and `prepare_line` |
| Seq | at most `i64::MAX`, strictly increasing over valid records | Rust bound |
| New stream facts | task status 256 bytes, EOF seq at most `i64::MAX` | shared with the fixture writer |

Refusals, all with nothing committed (the transaction rolls back): `TaskMissing` for a missing or damaged task;
`EventStreamClosed` for an existing stream whose row stores an EOF seq (its task finished; the
stored column is read, not the value the reads derive from the last record); `LimitExceeded` for
a bound above or a span past `i64::MAX`; `InvalidEventQuery` for an empty line, a body that is not
UTF-8, or new-stream facts naming an EOF seq when the batch does not end on the `task_finished`
event with that seq; `EventOutOfOrder` for a seq that does not increase; `Incompatible` for a
stream with a partial tail or whose last record does not end at `complete_end`. The event bounds
are checked before the transaction opens and the task, stream and EOF checks before any row is
inserted; a span past `i64::MAX` and `EventOutOfOrder` are found while the batch is inserted. The
stream re-read at the end of the append stays as a defensive check (`EventReadUnavailable`).

The fixture `append_event_record_fixture` routes a `FixtureLine::Event` after a complete line
(no partial tail) through the same append. The fixture's per-line validation and per-line
`Limits` check run first, so an over-long body or kind, a seq beyond `i64::MAX` or a non-UTF-8
body is refused with `InvalidEventQuery` there rather than the writer's `LimitExceeded`. The
fixture's stream behaviour is applied before the append: the stream facts are refreshed from the
task's facts on every append (which clears a stored EOF seq, so a fixture append to a finished
stream is still accepted), and appending to an unavailable stream rotates its generation.

The rows stored for every accepted append are identical to the earlier fixture-only path, and the
seed parity comparison is unchanged. What such an append no longer does, compared with that path:

- no cumulative `Limits` row or byte cap over the whole log: appends past the 128-row default
  succeed;
- no full re-validation of the existing records: damage in the middle of the log is no longer
  `Incompatible`; only the last record's end and the final stream re-read are checked;
- a duplicate or regressing seq is refused with `EventOutOfOrder`;
- an event with an empty body (`raw_len` 0) is refused with `InvalidEventQuery`;
- with no prior stream, orphan records of the task are refused as `Incompatible` instead of being
  deleted.

Malformed and partial lines, an event joining a partial tail, truncation and removal stay
fixture-only damage writers.

## Artifact writer

`Store::register_artifact` (`crates/hf-storage/src/artifact_writer.rs`) appends one metadata-only
reference to a valid task's manifest in one commit. The commit rotates the manifest revision (which
binds artifact cursors) and publishes nothing else: the dataset generation and the watch revision
stay, as for the reference manifest write. A task without a manifest gets one with a valid header.
The new item takes the next ordinal.

The bound is the reference's serialized size: `json.dumps(manifest, ensure_ascii=False)` with the
default separators, at most 1,048,576 bytes. The store does not encode JSON. The interface layer's
codec (`hf-interfaces` `artifact_codec`) computes the envelope length (this task's manifest with an
empty `items` list, with the engine's product identity) and the new item's length; the caller passes
both as `ManifestLengths`. Each item stores its length in `artifact_items.encoded_len`, and the
transaction sums the stored lengths in SQL, so the check is

    envelope + Σ stored encoded_len + new item + 2 × (stored item count)

against 1,048,576 (two bytes, `", "`, between consecutive items). Admission on a `Store` may
refuse a second concurrent writer with `Busy`; admitted writes serialize on the immediate
transaction, so the sum covers every committed item.

`Registrant` selects the reference's liveness and duplicate rules:

| Registrant | Task | Duplicate | Reference |
| --- | --- | --- | --- |
| `Caller` (a verified task process) | valid, with a header row, recorded status not terminal | same path, kind, note and observation (device, inode, size, mtime) | `artifacts.register` |
| `Engine` (engine output, observed backend plans) | valid, any status | same path and observation | `record_engine_output`, `record_backend_plan` |

A duplicate returns `Registered::Existing` with the first equal stored item, and nothing is written,
published or committed (`commit_sequence` does not advance). Otherwise the result is
`Registered::Added`.

| Bound | Value | Source |
| --- | --- | --- |
| Items per manifest | 100 | reference cap |
| Serialized manifest | 1,048,576 bytes, measured as above | reference `MAX_BYTES` |
| `path`, `note` | 4096 bytes each | reference `_text`; the reads refuse longer |
| `artifact_id`, `kind`, `registered_at`, method together | 1024 bytes | the reads refuse longer |
| `tool_use_id`; plan directory | 1..=256 bytes; 1..=4096 bytes | schema checks |
| Observation size; `encoded_len` | at most `i64::MAX` | SQLite integer |

Refusals before the transaction, with `LimitExceeded`: the text bounds above, plan evidence that does
not match the method, a size or item length
beyond `i64::MAX`, or an envelope and item that alone exceed the manifest bound. Inside the
transaction, in this order and with nothing committed: `TaskMissing` for a missing or damaged task
(for `Caller` also a task without a header row); `TaskFinished` for a `Caller` registration whose
task's recorded status (the header's observation column, else the facts status) is `completed`,
`failed`, `timed_out` or `cancelled`; `Incompatible` for a stored manifest whose header is invalid;
then the duplicate lookup; then `LimitExceeded` for a manifest already holding 100 items or a
serialized size over the bound. A `plan` must be present exactly for `observed_backend_plan`; any
other combination is refused before the transaction with `LimitExceeded` (the writer has no
separate shape error; the schema check stays as a backstop). Kind and method values, the
lengths' correctness and the target's metadata are the caller's.

The fixture `replace_artifacts_fixture` replaces a manifest in one commit through the same manifest
upsert and item insert, without the duplicate, cap or size checks (fixtures seed over-cap and
damaged manifests to exercise the reads), and stores each `FixtureArtifact::encoded_len` as given;
the seed tool computes it with the same codec.

## Tables read by the public ctl commands

`harflow-ctl` reads only through the `ReadSession` (see the [read-path design](store-read-path.md)),
inside one snapshot per command. Reading a table is not a guarantee that a writer produces it:
until E2 the rows come from fixture writers, and the reads prove behaviour on seeded data only.
Statements are in `crates/hf-storage/src/statements.rs` and the `*_reads.rs` modules; the
mapping below is by statement owner, not an exhaustive per-command contract.

| Tables | Read by |
| --- | --- |
| `task_dataset` (generation), `task_headers` (header and status blobs, the served bytes) | every task read; the dataset generation binds list and page cursors |
| `task_facts` (typed task facts, `valid` flag, `chain_root`) | `list`, `status`, `status-batch`, `task-list-page`, interaction and statistics feeds, caller candidate scans |
| `task_processes` | caller gate and the recovered/observation projection |
| `task_prompts` | `task-prompt` and the brief preview |
| `artifact_manifests`, `artifact_items` | `task-artifacts` and the artifact summary in `status` |
| `event_streams`, `event_records` | `task-events`, `status --brief`, `recovery-activity`, event token |
| `chain_stops` | `continuation_stop` on `status`, `status --brief` and `status-batch` |
| `launch_attempts` | `task-pending-page`, `task-pending-status` and the ordinary reads of a launch receipt |

`task_watch_meta` and the ownership, capture and conversation tables are not read by any
public ctl command. Only the internal private-copy `RecoveryReadSession` keeps a 64MiB cap; `Store::open`,
`recover`, `probe` and the read session have none.

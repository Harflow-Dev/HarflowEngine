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

Probe never opens the source using SQLite. Under lifecycle and I/O gates, copy
only the bounded main database and WAL to an isolated private scratch directory,
never source SHM. SQLite validates/replays that private copy; compare logical
application/schema/generation against the marker before source writable access.
The combined main+WAL snapshot cap is 64MiB for open, recover and probe, and it stays in force until E2 (public ctl reads use the read session, which has no cap of its own); chunk/deadline checks fail closed. Qualification
failure never authorizes source recovery. No custom WAL parser, immutable live-WAL
shortcut, sidecar deletion or original-file reset is used.

“No source writes” means no source content/sidecar creation/removal by probe;
ordinary file reads may affect access metadata. A read session may create or
rewrite `-shm`, never main or WAL content and never `-wal`. Probe may allocate private scratch. Scratch parents equal to or beneath the source
root refuse before allocation, including normalized aliases. Defaults derive from
the absolute source root parent, preserving relative-root support.
A missing root returns absent before scratch or lock creation. Foreign, unknown,
future, corrupt and mismatched identities refuse before any original SQLite open.
Normal scratch errors clean up through RAII. SIGKILL cannot run destructors: tests
supply invocation-owned scratch parents and clean exact remnants after child exit.
There is no broad scan or automatic stale-directory deletion.

Normal `open` is explicitly an internally authorized mutating operation: after
snapshot qualification under the I/O gate, it may let SQLite recreate SHM/WAL-index
and recover legitimate committed WAL. It never converts a probe into recovery.
Explicit `recover` additionally requires exclusive lifecycle ownership and requests
SQLite TRUNCATE checkpoint. Live Store handles block that maintenance rather than
being forcibly closed. Nonempty rollback journals refuse; this is a WAL-only recovery primitive. No
schema migration operation is added; future schema-change
work must use exclusive lifecycle, preserve generation/provenance and be separately
assigned. Unknown outcomes do not permit reset/replay.

## Read session

A read session (`ReadSession`) answers reads from the real store without a copy.
It takes `store.lock` shared (acquisition capped at 2s) and never `io.lock`, and
it writes no main or WAL content. Before SQLite is opened it checks the marker,
the database header, that the rollback journal is empty, and that `-wal` exists
and is a regular file (`lstat`); a missing `-wal` is refused because opening would
create an empty one and report an empty history. A `-wal` deleted between that
check and the open is not detected. It then opens with `SQLITE_OPEN_READ_ONLY` and
checkpoint-on-close disabled, compares the database identity with the marker, and
runs the existing queries in one read transaction. It runs no `quick_check` and has
no size limit of its own. The deadline is soft: SQLite open and WAL recovery cannot
be interrupted, so a read that overruns is refused after the fact as busy; the
caller supplies 5s for a read. Any SQLite error is a refusal, and `ReadRefusal`
classifies refusals for the transport above. Open, recover, probe and the capped
copy are unchanged and keep the 64MiB cap, `quick_check` and `io.lock`.

## Checkpoint and close policy

Source connections disable automatic checkpoints and checkpoints on close before
SQL access, using `wal_autocheckpoint=0` and the safe rusqlite wrapper for SQLite
`SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`. WAL/FULL/foreign keys remain checked. This avoids
an uncoordinated last-close checkpoint while another process copies source files.
The private qualification connection may recover its copy; it has no source handle.

PASSIVE checkpoint runs behind handle admission and the I/O gate. It may coexist
with readers and must return frame counts and completeness truthfully: a zero busy
indicator is not proof that all frames were checkpointed. Exclusive recovery uses
TRUNCATE only after other lifecycle holders are gone; incomplete/busy results are
observable, never permission to delete WAL or force reader release. Disabling auto
checkpoint requires an eventual scheduling/WAL-growth policy; this suffix supplies
manual primitives and a bounded qualification cap, not a production scheduler or
ratified performance result.

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
64MiB qualification, nonmutating probe/source-open distinction and explicit recovery
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
manual-checkpoint and64MiB qualification limits for open/recover/probe remain unchanged.

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
present exactly for that method, and the directory columns are all present or all absent. Schema7
has not been released, so these columns did not bump the version.
Header/status blobs remain the served bytes. Schema6 and schema5 roots refuse as
`unsupported_layout`, without upgrade or repair; no checked-in database fixture or
internal root depends on an older schema. Open/recover/probe retain the 64MiB cap; public ctl
reads use `ReadSession`, including `recovery-activity`. The internal operation-scoped
`RecoveryReadSession` remains a distinct capped private-copy path.

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
public ctl command. The 64MiB cap above still governs `Store::open`, `recover` and `probe`
(and the internal private-copy `RecoveryReadSession`) until E2; the read session has no
size cap of its own.

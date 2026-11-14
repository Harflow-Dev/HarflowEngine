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
exists and is a regular file (`lstat`), then the lost-WAL acceptance record, the fence and
the commit watermark (below).
A missing `-wal` is refused (`WalMissing`) and none is created, because SQLite would
create an empty one and report an empty history.
They then open the source with `SQLITE_OPEN_READ_ONLY` and checkpoints suppressed,
compare the logical application/schema/generation with the marker and the database
sequence with the watermark, and re-check the
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

Race handling: the storage reader checks the file with `lstat` and then opens it with
`O_NOFOLLOW|O_NONBLOCK` on macOS and Linux (since E2-A slice A6, shared with the fence and due
record; the flag values are spelled out because hf-storage has no libc dependency), so a FIFO
swapped in between the two calls is opened without blocking and refused as `Incompatible` by the
`fstat` after opening, and a symlink swapped in fails the open with `ELOOP`, which surfaces as
`Error::Io` (in the read session `ReadUnavailable` naming the file), never followed. Other unix targets open without those flags and keep
that race; the marker read is unchanged. `state-inspect` opens it with `O_NOFOLLOW|O_NONBLOCK`.

## Verification and fence

Owner decision A2 keeps a cheap identity check on every open and moves the whole-store
`quick_check` to recover, probe and a per-generation cadence (`crates/hf-storage/src/verify.rs`).
E2-A ships the storage primitives; the maintenance actor that runs the cadence and clears a fence is
E2-B.

The due record `store.verify` holds `harflow-verify-1\n<generation>\n<due_unix_s>\n`, at most 128
bytes, read with the same `lstat`-first, bounded, device/inode-checked reader as the acceptance
record. `Store::verification_due(now)` is true when the record is absent, malformed (including a
regular file over 128 bytes, which the next pass replaces) or names another generation, when `now`
has reached the due time, or when the due time lies more than one period (`VERIFY_PERIOD`, 24
hours) ahead of `now` (a clock that moved back). A non-regular record (directory, link, FIFO) is
`Incompatible`. `Store::verify_if_due(now, deadline)` runs one check when it is due.

`Store::verify_in_place(deadline)` refuses an already-past deadline with `DeadlineExceeded`, then
checks the handle's store under the handle's shared lease and
without `io.lock`: the read-order prechecks (marker, header, journal, `-wal`, acceptance record,
fence, watermark), a read-only connection with checkpoints suppressed, the SQL identity, the
watermark comparison (`CommitLost` when the database is below it), then one
`PRAGMA quick_check(1)` through the bounded scan helper (the watchdog interrupts it at the
deadline). Before recording a verdict it re-checks the binding, the `-wal` identity and the marker.
The verdict:

| `quick_check` result | Verdict | Effect |
| --- | --- | --- |
| `ok` within the deadline | `Passed` | `store.verify` rewritten with due time `now + 24 h` |
| any other verdict, `SQLITE_CORRUPT`, `SQLITE_NOTADB`, whenever it arrives (also after the deadline) | `Fenced` | `store.fence` written |
| `ok` after the deadline, interrupted at the deadline, busy, `SQLITE_IOERR`, or a failure to set the busy timeout before the scan | `Inconclusive` | nothing written; due time unchanged |
| any other SQLite error | returned as the error | nothing written; due time unchanged |

A failed prerequisite (a refused precheck, an identity mismatch, a replaced binding) is returned as
its error and records nothing. A passing scan whose due-record write fails (the lock budget runs out,
or an I/O error) returns that error: nothing is recorded and the store stays due. Corruption raised by the identity query itself is `Incompatible`, not
a fence: every open refuses such a store anyway. `commit_sequence` is not consulted: it counts
commits, including no-op commits, and is not a change detector.

Both records are written only under `io.lock`, which is taken after the scan with its own budget
(`RECORD_BUDGET`, 2 s from the verdict) so that a failure found near the scan deadline is still
fenced. The lease port does not wait for a held lock (in process it answers `Busy` at once, across
processes the lock is a non-blocking `F_SETLK`), so a busy `io.lock` is retried every 5 ms until the
budget runs out. After a corruption verdict, any failure to record the fence (the lock budget
running out, an I/O error writing, renaming or flushing it, or a read-back mismatch) is
`Error::FenceUnrecorded` carrying the cause: corruption was found, nothing records it and the due
time is unchanged, so the next due check scans again. A refusal of an existing fence (below) is not
wrapped. The lock is held for one small file write, so the verifier never holds a commit for the
length of a scan; a commit that collides with that write can still get a short `Busy` from its own
`io.lock` acquisition. Before writing the fence the verifier checks the existing one under the
lock: absent or this generation's `failed` proceeds, anything else (a foreign generation, malformed
content, a non-regular file) is returned and left in place. The due-record write likewise refuses
when a fence is present. Writes go through `<name>.tmp`: a stale temporary file is removed, a new one is
created exclusively (a planted link is never followed), filled and renamed over the target, so a
reader sees the old or the new content, never a torn mix. The fence is durable: the temporary file
is flushed with `sync_all` (which issues `F_FULLFSYNC` on Apple platforms) before the rename and the
directory is flushed after it (`File::open(dir).sync_all()`, `EINVAL` tolerated as SQLite does;
unix only, since non-unix roots are refused at binding). The due record is not flushed: losing it
only repeats a check. Neither file is created at initialization, so seeded roots never contain them.

`store.fence` holds `harflow-fence-1\n<generation>\nfailed\n`, at most 128 bytes, read like the
acceptance record. All three files are opened with `O_NOFOLLOW|O_NONBLOCK` on macOS and Linux (x86,
x86_64, arm, aarch64; flag values spelled out because hf-storage has no libc dependency), so a FIFO
swapped in after `lstat` is refused as `Incompatible` by the `fstat` that follows instead of
blocking, and a symlink swapped in fails the open with `ELOOP` (`Error::Io`; in the read session
`ReadUnavailable`) instead of being followed; other unix targets open without those flags and keep
that race. `failed` for the marker's generation is `Error::Fenced`; a foreign generation, malformed
or oversized content, a non-regular file, or a regular file replaced by another one after `lstat`
(device/inode mismatch) is `Incompatible`. The check runs after the acceptance record, in the refusal order marker, header,
journal, `-wal`, acceptance record, fence: in `qualify::precheck` and again at the end of in-place
qualification (so `open`, `recover`, `probe` and the verifier refuse), in `Store::write` after
`io.lock` and before `BEGIN` (no retry; every later commit refuses, including WAL maintenance
writes), in WAL maintenance under `io.lock` before a PASSIVE checkpoint (a fenced store is skipped,
not checkpointed), and in the read session at open, at every read entry and at every read exit. A running
handle's commits therefore refuse from the first commit after the fence lands; reads already inside
a snapshot finish and are then refused at exit. `state-inspect` reads `store.fence` lease-free,
bounded and `O_NOFOLLOW`, after the acceptance record, and reports `incompatible` with
`infrastructure_unavailable` for `failed` in the current generation or an unreadable fence and
`invalid_marker` for anything else. Clearing the fence (after an explicit verify or recover) and
the owner runtime's behaviour while fenced belong to E2-B; the clearing path needs a precheck
variant, because `recover` and `probe` refuse a fenced root today.

## Commit watermark

`store.watermark` in the state root records the highest `commit_sequence` value whose commit was
acknowledged (`crates/hf-storage/src/watermark.rs`; codec in `crates/hf-application/src/watermark.rs`,
shared with `state-inspect`). A database whose sequence is below it has lost acknowledged commits,
for example after a restored or truncated `-wal`; SQLite alone reports such a store as healthy.

Format: exactly 8192 bytes, two 4 KiB blocks, slot A at offset 0 and slot B at offset 4096. Each
block starts with a 64-byte payload, little-endian: `[0..8)` magic `HFWMARK1`, `[8..40)` the
generation as 32 lowercase hex characters, `[40..48)` the sequence as u64, `[48..60)` zero,
`[60..64)` CRC-32 (IEEE, as zlib) of `[0..60)`; bytes `[64..4096)` of the block are zero. A block is
valid when magic, CRC, padding and filler all hold; an invalid block is a torn or unfinished write
and is ignored. A valid block naming another generation, a length other than 8192 or a non-regular
file is `Incompatible`; no valid block, a missing file or an unreadable one is
`ReadUnavailable("store.watermark")`. The best valid block holds the highest sequence; on equal
sequences slot A is the best.

Initialization (`Store::initialize_fixture_with`) commits the schema at sequence 0, then creates
the file with `create_new`, both slots at sequence 0, flushes it through the options' `FlushPort`
and syncs the directory, then publishes the marker and syncs the directory again. Every marked
root therefore has a watermark; a crash before the marker leaves an unmarked root. Nothing
recreates the file afterwards.

Writer protocol, inside the commit path under `io.lock`, after the fence check:

1. Open the file read-write without following a link and without creating it (`lstat`, then the
   open, then a device/inode match), check the length and decode both blocks with positioned reads.
   A missing, malformed or foreign file fails here, before `BEGIN`.
2. Flush the file as it stands (the pre-flush). A writer that died between its slot write and its
   flush can leave a best slot that is only in the page cache; flushing before the next overwrite
   makes it durable first (owner decision 2026-11-14, flush before overwrite, on every commit).
3. `BEGIN IMMEDIATE`, then compare: a decoded sequence above the database's `commit_sequence` is
   `Error::CommitLost`, before the action runs.
4. Action, publication, sequence increment (`UPDATE ... RETURNING` gives the new value n), binding
   check, commit, binding check.
5. Write n into the slot that does not hold the best entry (when both are valid, the lower one;
   on a tie, slot B), then flush again. The whole 4 KiB target block is written (payload and zero
   filler) in one positioned write, so damaged filler in that block is repaired. The best entry is never overwritten before a newer one has
   been written and flushed, so a torn write can only damage the target block.

Failures: steps 1 and 3 return their own error and commit nothing. A failed pre-flush poisons the
handle and returns `Error::WatermarkUnrecorded` with nothing committed. A lost binding after the
commit returns `CommitOutcomeUnknown` before the slot write and does not poison: the database is
one ahead of the file, which reads and reopen accept. A failed slot write or post-flush returns
`CommitOutcomeUnknown` (the commit is visible but its durable acknowledgement is not established)
and poisons the handle. A poisoned handle refuses every later write with `WatermarkUnrecorded`,
checked right after admission, because a flush retried after a failure can report success without
the data being durable; reads and PASSIVE checkpoints on it continue, and reopening the store is
legal because the database is ahead of the file. WAL maintenance does not treat
`WatermarkUnrecorded` or `CommitLost` as a skip; the maintenance shrink commit goes through the
commit path and writes the watermark itself (a failure there is discarded by the best-effort
maintenance call and surfaces on the next write). PASSIVE checkpoints do not change the sequence
and write nothing. Test-only stage hooks `watermark_before_write` and `watermark_before_flush` fire
before the slot write and before the post-flush, whatever the commit's stage label.

`FlushPort::flush(&File)` flushes the descriptor the writer holds, never a path. Production
handles get their port from `hf_platform::durability`: `FullFlush` is an explicit
`fcntl(F_FULLFSYNC)` on macOS and `sync_all` elsewhere; `FsyncOnly` is a plain `fsync(2)` on
macOS (std's `sync_all` and `sync_data` both issue `F_FULLFSYNC` there) and `sync_all` elsewhere.
Neither falls back to the other: at the port, a `FullFlush` the file system or device refuses
(`ENOTSUP`, `ENODEV` or any other error) returns that `io::Error` unchanged and is never retried as
`fsync`. The store then fails closed, by where the flush runs:

- New root: `Store::initialize*` creates the file through `watermark::create`, whose flush error
  surfaces as `Io`. The marker is not published, so the root never becomes an openable store.
  `store.sqlite3*` and a partial `store.watermark` remain, so the root must be cleared before
  initialization is retried: a retry on the same path is refused as `InvalidRoot` because the
  directory exists (and `watermark::create` itself would refuse the existing file, `create_new`,
  as `Io`).
- Existing root, pre-flush (`settle`, before any slot write): the handle is poisoned and the write
  returns `WatermarkUnrecorded`; nothing is committed. On a volume without `F_FULLFSYNC` this is
  where every write stops.
- Post-write flush (the pre-flush succeeded): the commit is visible, the write returns
  `CommitOutcomeUnknown` and the handle is poisoned.

`DEFAULT_DURABILITY` is `FullFlush` (since A7b). Only the host's writer handles take it
(`hf-host/src/durability.rs`). The fixture seed uses `FsyncOnly`, because seeded roots are test
data, not acknowledged history. Handles opened inside hf-storage with `WriterOptions::suppressed()`
or `owner()` unchanged (for example the read-only watch session in `task_watch.rs`) and
`Store::open` keep hf-storage's own default, `SyncAllFlush` (`sync_all`).

Measured at A7b (Apple M3 Pro, internal APFS): `create_task` through a fresh writer child with
`FullFlush` and `Suppressed` has write p99 about 14 ms (under 20 ms). That figure was taken with
the store reset every 50 rounds (`HF_WL_RESET=50`): under `Suppressed`, `-wal` otherwise grows
without bound (about 82 KB per `create_task`) and a fresh writer's open time grows with it, so the
budget holds only once E2-D's WAL maintenance bounds `-wal`. One writer sustains about
136 commits/s. Under four concurrent tasks the commit-lag p99 is 161 to 506 ms, driven by the
non-waiting `io.lock` with 5 ms `Busy` polling. E2-D entry conditions: a waiting or queued
`io.lock` hand-off replaces the polling, the capture-publication commit is coalesced into the
event append commit, and commit-lag p99 is below 100 ms for the r06/r07 four-task burst and the
4 x 42 lines/s sustained profile under `FullFlush`; idle-loop WAL maintenance bounds `-wal`
growth before the owner runtime uses the store.

Readers hold no `io.lock`. They read each block with its own positioned read; when neither is valid
they retry the pair up to three more times, 1 ms apart (a reader can see one block before a commit
and the other torn by the next), then report `ReadUnavailable("store.watermark")`. The file is read
before the database snapshot is taken and compared after the identity read: a database sequence
below the watermark is `CommitLost`, a sequence at or above it is accepted (a concurrent commit
legitimately moves the database ahead of the file). The check runs after the fence, in the refusal
order marker, header, journal, `-wal`, acceptance record, fence, watermark (file faults before
`CommitLost`): in `qualify::precheck` with the comparison on the qualification snapshot (so `open`,
`recover` and `probe` refuse; `open` compares under `io.lock`, and the A0 tamper case of a missing
`commit_sequence` table stays `Incompatible`), in the verifier on its read-only connection after
the identity check (a `CommitLost` store is refused before the scan, and neither the due record
nor the fence is written), and in the read session at open and at every read entry. End-of-qualification and read-exit re-checks test presence only, never the
sequence. Not covered: a store handle's own reads (`Store::task_read`, `Store::event_read`,
`read_batch`) are checked at writer open only, and `RecoveryReadSession` (the capped private copy,
`recovery.rs`) checks neither the fence nor the watermark. `state-inspect` reads the file lease-free and bounded (8192 bytes,
with the same retry) after the fence: absent, unreadable or no valid block is
`infrastructure_unavailable`; non-regular, oversize, a wrong length or a foreign generation is
`invalid_marker`. It opens no database, so it cannot see a lost commit: such a root reads `ready`
there and is refused by the read session. Empty and lock-only roots stay `ready`.

Residuals, documented rather than closed in E2-A:

- Media corruption of the newest block lowers the detection threshold to the older entry, which
  can be more than one commit back after interrupted publications, instead of refusing.
- Write ordering (W1, shipped): SQLite `synchronous=FULL` issues plain `fsync` for WAL frames on
  macOS, and the writer does not full-flush `-wal` itself. The acknowledged-implies-durable argument
  assumes that `F_FULLFSYNC` on the watermark descriptor flushes the whole device cache, including
  WAL frames already handed over (APFS on internal NVMe). If a device persisted the slot write
  without the earlier WAL frames, a power loss could leave slot n without WAL frame n: a false
  `CommitLost` for an unacknowledged commit, which fails closed and needs E2-B rollback acceptance
  to clear. Under `FsyncOnly` (fixture seed) neither barrier is a device-cache flush, so the same
  holds more broadly. Flushing the `-wal` descriptor before the slot write (W2) was measured at A7b
  (about +3 ms p50 and +5 ms p99 per commit, p99 at the 20 ms budget line) and not adopted.
- A process kill between commit and slot write leaves the database one ahead of the file; the next
  commit catches the file up. That state is legal and readable.

E2-B entry conditions: rewriting the watermark down to the database sequence when a rollback is
accepted; recreating the file when it is missing or both blocks are invalid; and a precheck variant
so that `recover` and `probe` can run on a `CommitLost` root (today both refuse, as for the fence
and the acceptance record); and `RecoveryReadSession` checks the fence and the watermark before the
owner runtime uses recovery observation.

## Read session

A read session (`ReadSession`) answers reads from the real store without a copy.
It takes `store.lock` shared (acquisition capped at 2s) and never `io.lock`, and
it writes no main or WAL content. Before SQLite is opened it checks the marker,
the database header, that the rollback journal is empty, and that `-wal` exists
and is a regular file (`lstat`), the lost-WAL acceptance record, the fence and the commit
watermark (above); a missing `-wal`
is refused because opening would create an empty one and report an empty history. A `-wal` deleted between that
check and the open is not detected. It then opens with `SQLITE_OPEN_READ_ONLY` and
checkpoint-on-close disabled, compares the database identity with the marker and the
database sequence with the watermark, and runs the existing queries in one read transaction. It runs no `quick_check` and has
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
`flush`, a `FlushPort` (`hf-application::durability`), makes the commit watermark durable
(see [Commit watermark](#commit-watermark)); `suppressed()` and `owner()` default it to
`SyncAllFlush` (a no-op in hf-storage's own unit-test build) and `with_flush` replaces it. Production handles pass the platform port for
`DEFAULT_DURABILITY`.

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
(`crates/hf-storage/src/write_tx.rs`), in this order: admission, the poison check, `io.lock`
(exclusive; the lease port never waits, so a held lock is `Busy` at once), the fence check, the
watermark open, decode and pre-flush, a `BEGIN IMMEDIATE` transaction, the watermark comparison,
the caller's action, the publication the caller asked for, the `commit_sequence` increment, the
binding check, the commit, a binding check after it, and the watermark slot write and flush (see
[Commit watermark](#commit-watermark)). A commit failure is `CommitOutcomeUnknown`, and so is a
failed post-commit binding check or watermark publication. The publication is
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

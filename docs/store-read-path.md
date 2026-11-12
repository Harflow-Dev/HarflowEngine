# Store read path and writer open (E1 slice S2), revision 3

Status: read session implemented; the revision-3 measurements below remain historical
S2 evidence, not release-binary budget acceptance. Current macOS arm64 ctl reads use
`ReadSession` and one snapshot per command; fresh roots use schema 7, and schema 6 and schema5
reads refuse as `unsupported_layout` without migration. The
[E1 receipt](migration-receipts/E1.md) records coordinator-verified release
evidence and outstanding gates. E1 has not exited or been accepted.
The protocol and its amendments are in
[measurement protocol](store-read-measurement.md); guarantees named here are in
[storage protocol](storage-protocol.md). Raw data is retained outside the
repository under `.claude-work/engine/e1/s2-measure/`. Hardware: Apple M3 Pro,
11 cores, macOS 26.6.2, APFS, rustc 1.99.1, release build, bundled SQLite
(rusqlite 0.41.1).

Evidence status. Latency and sidecar-semantics numbers were gathered with the
first harness. Review found weaknesses in four evidence items (killed-reader
attribution, corruption comparison, contention accounting, candidate SQL
decoding); the harness was corrected (protocol amendments 9 to 13) and those
items were rerun on 2026-11-07 at load average 3.3 to 5.1 (every rerun batch
below the 5.5 threshold; per-batch loads in `batches.jsonl`). Results that
changed a first-run claim are marked **changed**.

## Scope: what is implemented at E1 and what is only specified

Two parts, kept separate throughout:

- **Implemented in S2.** The read session (a new read-only way to
  answer reads) and the `state-inspect` `-wal` row. Subsequent E1 slices compose
  the public read-only ctl commands with it: S3 to S5 put all ten store-backed
  reads on it, and S5 adopted it for `recovery-activity`, which no longer uses the
  private-copy path. At E1, `Store::open`, `recover`,
  `probe`, `probe_with_scratch`, `open_with_scratch` and the capped copy stayed as they
  were. E2-A slice A1a replaced them: open, recover and probe now qualify in place
  (Part 2), `probe_with_scratch` became `probe_with_deadline`, `open_with_scratch`
  and the `scratch_parent` argument were removed, and only `RecoveryReadSession`
  keeps a capped private copy. Measurements below describe the E1 paths.
- **Specified for E2, not implemented.** The replacement of the 64 MiB limit for
  writer open and recover, which E1's exit requires as a design, together with
  checkpoint ownership and qualification policy. Several choices are named
  decisions that this document does not settle.
- **Owner decision D20 (2026-11-07), accepted for E1 only.** Silent rollback of
  history on WAL damage is accepted for E1, which has no public writer. This is not
  a guarantee that damaged fixtures cannot meet it. The commit watermark (below) is
  a named E2 decision and is carried there with the generation write sequence and
  the recovery signal.

## Decision

Keep SQLite and the lock hierarchy. The read session opens the real store
read-only under a shared `store.lock` lease, with no private copy, no per-call
`quick_check`, no `io.lock` and no size limit of its own; the caller's deadline
bounds acceptance of the answer, not elapsed time (below). It refuses typed, with
existing contract codes, when the store is not in a legitimate engine state. For
writer open and recover the document specifies, for E2, the replacement of the
64 MiB limit.

## Measured facts

### Existing path (copy, `quick_check`, read-write open)

Operation time (open plus query) p95 in ms, fresh process per sample (the
operating system file cache is not controlled), repetitions r1/r2/r3 (* r1 ran
partly above the load threshold of 5.5):

| store | main / WAL | p95 r1 / r2 / r3 | per kind status/list/events, r2 and r3 |
| --- | --- | --- | --- |
| ci `ckpt` | 23.8 MiB / 0 | 30.1* / 25.6 / 23.4 | 24.9/26.5/25.1 ; 28.0/23.4/23.1 |
| ci `tail` | 23.8 / 5.5 | 38.5 / 32.4 / 28.9 | 34.9/31.0/35.7 ; 28.9/28.1/28.9 |
| ci `wal` | 0.1 / 44.2 | 65.2* / 79.6 / 54.8 | **113.4**/79.6/76.2 ; **72.2**/48.4/51.5 |
| large, all three states | 397 / 0, 397 / 5.5, 0.1 / 750 | refused `LimitExceeded`, 100% of 200 | not servable |

So on the CI `wal` store the existing path misses the 50 ms status budget in
every repetition (status p95 113, 73, 72 ms in r2, r1, r3), while list and
events (100 ms budget) are within it in r3 and borderline in r2.

Cost breakdown (median of 90 replays, ms): CI `wal` copy WAL 11.7, WAL recovery
on the copy 9.7 and on the source 9.8, `quick_check` 12.0, total 44.5; large
`ckpt` copy 113, `quick_check` 184 (0.46 ms per MiB of main file), total 317.
Query cost is 0.2 (status), 0.6 (list), 0.7 ms (events). The CI `wal` store
(30,000 events, 18 MiB of bodies) is 44.3 MiB of the 64 MiB limit because WAL
frames roughly double the data (1.9 times at the large scale).

A PASSIVE checkpoint does not shrink the WAL file; with `journal_size_limit`
the file shrinks at the next write after a complete checkpoint (measured: 46.4
MiB stays 46.4 MiB without the limit, drops to 4 MiB with it). Ten seconds of
fixture writes with checkpoints every 50 commits left a 24.9 MiB main file and a
44.5 MiB WAL (71.6 MB), and `Store::recover` refused with `LimitExceeded`.

### Candidate path (read-only open, shared lease only)

Lease, marker, header, `SQLITE_OPEN_READ_ONLY`, no checkpoint on close,
identity query, then representative SQL. Note: the representative SQL decoded
only column 0 of each row in the runs below; the harness now decodes every
column (amendment 10). Query-phase figures are therefore lower bounds, open-phase
figures (the large part) are unaffected. p95 in ms, r1/r2/r3, fresh process:

| store | WAL | p95 | per kind status/list/events (r2 ; r3) |
| --- | --- | --- | --- |
| ci `ckpt` | 0 | 1.3* / 0.7 / 0.6 | 0.7/0.7/0.7 ; 0.7/0.6/0.6 |
| ci `tail` | 5.5 MiB | 2.3 / 1.9 / 1.8 | 1.8/1.8/1.9 ; 1.8/1.8/1.8 |
| ci `wal` | 44.2 MiB | 11.7 / 10.4 / 10.0 | 10.7/10.5/10.3 ; 9.9/10.1/10.0 |
| large `ckpt` | 0 | 1.5 / 1.1 / 0.7 | 0.9/0.7/1.1 ; 0.7/0.7/0.7 |
| large `tail` | 5.5 MiB | 2.6 / 3.2 / 1.8 | 2.9/2.6/3.5 ; 1.8/1.7/1.8 |
| large `wal` | 750 MiB | 161 / 161 / 163 | 160.6/162.7/163.2 ; 167.1/160.5/163.5 |
| ci `wal`, a connection held open | 44.2 MiB | 0.4 / 0.6 / 0.4 | |
| large `wal`, a connection held open | 750 MiB | 0.7 / 1.3 / 0.6 | |

Rule: a cold reader that is the first connection rebuilds the wal-index by
scanning the whole WAL, about 0.21 ms per MiB (44 MiB and 750 MiB agree). A
stale `-shm` is not trusted (rebuilt even when copied alongside). With a live
connection held the rebuild disappears (0.4 to 1.3 ms for 44 and 750 MiB WALs).
Fresh-process overhead (null child) is 2.1 ms p50, 3.0 p95, 3.4 p99 (600
samples), reported separately; end-to-end p95 of the owner-maintained cases is
under 6.2 ms. The "held connection" rows come from a reader process, not an
owner with its own write transaction; the owner case is unmeasured. Without any
holder, concurrent cold readers still reuse each other's wal-index (large `wal`
p50 0.6 ms, p95 176 to 180 ms, see contention).

### Contention (rerun; 3 repetitions c1 to c3, all unflagged, max load 5.09)

1 writer process and 4 concurrent cold readers, 1,000 samples per reader (4,000
per run, 12,000 per setting over three repetitions), readers running 4.55 to
6.65 s (existing path) and 2.94 to 5.49 s (candidate) of the writer's life. Writer failures are counted inside the reader window, as
errors over attempts. **Changed:** the first run's "17 to 31 percent" was whole
life and short overlap; inside the window the existing path's writer failed
**95 to 98 percent of attempts** (the lowest is c1 with checkpoints, 112,635 of
118,626, 94.9 percent), so readers do starve the single writer. The writer has
no backoff, so attempt counts depend on how fast failures return: the existing
path's writer made about 17,000 attempts per second of which 380 to 900 per second
succeeded, against 5,800 to 9,300 commits per second for the candidate's writer;
readers completed 600 to 880 samples per second on the existing path and 730 to
1,360 on the candidate.

| path | reader failures | reader op p95 | writer failures in window |
| --- | --- | --- | --- |
| existing, ci `tail`, with or without writer checkpoints | 95 to 97% `Busy` (6 runs) | 0.7 to 0.9 ms (failures; the 3 to 5% that succeeded: p99 29 to 36 ms) | 95 to 98% of attempts; 380 to 900 commits/s |
| candidate, ci `tail` | 0 of 12,000 per setting | 0.6 to 0.9 ms | 0 of ~27,000 per run, 8,800 to 9,100 commits/s |
| candidate, large `tail` (raw writer) | 0 | 1.5 to 2.7 ms (one run with checkpoints, c1: 8.7, max 17.8) | 0 of ~28,000 to 35,000, 5,800 to 9,300 commits/s |

Concurrent cold readers, **no owner and no holder** (4 readers, 250 each):

| path | store | failures | op p50 / p95 / p99 (ms) |
| --- | --- | --- | --- |
| existing | ci `wal` | 96 to 97% `Busy` | 0.2 / 0.6 / 48 |
| existing | ci `tail` | 95 to 96% `Busy` | 0.2 to 0.3 / 0.6 to 0.9 / 27 to 29 |
| candidate | ci `wal` | 0 | 11.5 / 12.7 / 12.8 |
| candidate | ci `tail`, large `tail` | 0 | 2.0 to 2.1 / 2.5 to 2.9 / 2.8 to 5.6 |
| candidate | large `wal` (750 MiB WAL) | 0 | 0.6 / **176 to 180** / 185 to 219 |

Both `wal` rows are bimodal, and the waiters pay too. Whichever reader finds no
other connection open rebuilds the wal-index, and readers that start while it is
being rebuilt wait for it. CI `wal`: only 201 to 207 of every 1,000 samples were
under 5 ms (about 0.6 ms), the rest took about 11.5 ms. Large `wal`: 734 to 754
of 1,000 were under 5 ms, 246 to 266 took over 100 ms (about 25 percent), hence
a p95 of 176 to 180 ms and a p50 of 0.6 ms. With no owner a 750 MiB WAL therefore
misses the 100 ms budget at p95, and the verdict quotes 176 to 180 ms for
concurrent readers (161 ms is the sequential cold figure). This supports treating
WAL size as a reported health signal and having a writer role keep it small. The
existing path serializes even concurrent readers with no writer, so it fails 95
percent of concurrent cold reads at any store size.

### Sidecar semantics of `SQLITE_OPEN_READ_ONLY` (ci data; expected 30,000 events, 300 tasks)

| case | result |
| --- | --- |
| (a) `-wal` and `-shm` present: writable dir, read-only dir, read-only dir and files | correct; WAL bytes unchanged; no files created; `-shm` contents rewritten unless read-only |
| (b) nonempty `-wal`, no `-shm`, writable dir | correct; SQLite creates `-shm`; WAL unchanged |
| (c) same, read-only dir (files writable or read-only) | fails: `unable to open database file` (CANTOPEN); nothing created; no silent omission |
| empty `-wal` plus `-shm`, read-only dir | correct; empty `-wal`, no `-shm`, read-only dir: CANTOPEN |
| no sidecars, writable dir | correct; creates empty `-wal` and `-shm`; read-only dir: `attempt to write a readonly database` (READONLY) |
| `immutable=1`, with or without `-shm` | opens, 0 events 0 tasks: whole WAL silently dropped (negative control) |
| `-wal` deleted (f1 with `-shm`, f2 without) | opens, **creates an empty `-wal`**, reports 0 events and 0 tasks: silent history loss |
| `PRAGMA wal_checkpoint` on a read-only handle | errors; a reader cannot checkpoint |

### Corruption (rerun with full-row comparison)

Every column of every returned row is compared against rows regenerated from the
generator (not another store read); the pristine store matched that baseline
in both baselines (main and WAL). Read-only handle, ci `ckpt` store, 20 trials
each:

| damage | outcome |
| --- | --- |
| zeroed page | 17 refused by reads, 3 unaffected (unqueried pages); `quick_check` flagged 20 of 20; existing path refused at open in the 4 sampled (`Incompatible`) |
| one flipped byte in a page | **changed:** 13 unaffected, **7 silent wrong data** (a successful read returned a different row); `quick_check` said `ok` in all 20 |
| zeroed header page | refused (`file is not a database`); existing path `Incompatible` |
| file halved | refused (`malformed`); existing path refused |
| file shortened by 100 bytes | **changed:** silent wrong data; `quick_check` `ok`; existing path also returned a record that differs from the generated one |

Through the production queries on a qualified open store with one page zeroed
afterwards (30 trials, every event page query plus the status and list views
compared as complete values): 20 trials gave typed errors, **0** returned rows
different from the pristine store, **0** returned a record not in the generator,
and the status and list views were identical in all 30. A refusal, never empty
or different history, for structural damage a query touches. Payload damage is
different: SQLite has no page checksums, so a flipped byte is returned as data
and neither `quick_check` nor the old open-time scan detects it. For the existing
path this is an inference: its `quick_check` also said `ok` on every flipped
page and on the shortened file (where its sampled task returned a record
differing from the generated one), but the existing path's flip results were
sampled on two tasks only. The first run missed the effect because it compared
only counts and key columns.

WAL damage (ci `wal`, `-shm` removed; all nine cases): every case opens without
error, `quick_check` is `ok`, and the successful history is silently shorter:
15,000 of 30,000 events after the 50 percent cases (150 task pages differ),
29,900 after last-frame damage, truncation mid-frame or a zeroed last-frame
header (1 page differs), `Ok(0)` records for a task's events on the existing
path, and `TaskReadUnavailable` after header damage or truncation to the header.
A kept `-shm` made no difference. Identical to the existing path.

### Killed readers (rerun; remnants captured at the reap)

| case | trials | new files at reap | scratch dir | lease free | next read |
| --- | --- | --- | --- | --- | --- |
| reader holding a read transaction, `-shm` present | 1 | none | no | yes | correct |
| kill after open completed (handshake `READY`), `-shm` absent | 5 | `store.sqlite3-shm` in 5 of 5 | no | yes | correct |
| kill 0 to 8 ms after `BEGIN` (open started), `-shm` absent | 6 | `-shm` in 5 of 6 | no | yes | correct |
| kill 0 to 160 ms after `BEGIN`, 750 MiB WAL, `-shm` present | 7 | none | no | yes | correct |
| 12 to 168 ms after spawn (timing only), large | 14 | none | no | yes | correct |
| existing path, 2 to 60 ms after `BEGIN` | 30 | none | **22 of 30** `.hf-snapshot-*` | yes | correct |
| existing path, spawn-relative | 30 | none | 21 of 30 | yes | correct |

**Confirmed:** a reader killed while it is the one creating `-shm` leaves it
(derived state, harmless, now accepted by design). With `-shm` present no file
is ever left. The remnant count for the existing path varies by run (28 of 30
earlier, 21 to 22 now) because the window is the copy; the finding stands.
`-shm` and `-wal` retention (new experiment): after a clean close of an engine
handle with commits, after a read-only reader's close, after a reopen without
writes, and after `recover` (TRUNCATE) the files are `-shm` 32 KiB and `-wal`
8,272 bytes, 8,272, 8,272 and 0 bytes respectively. `-wal` is never deleted by
any of them, supporting "missing `-wal` is not a legitimate engine state".

### Concurrency safety

One writer, 54,157 commits in 10 s, a passive checkpoint every 50 commits (1,078
complete, 5 incomplete, 0 errors), against 4 reader processes holding only the
shared lease, with 3.49 million reads inside read transactions: 0 torn pairs,
0 read errors, 0 reads going backwards. `Store::recover` while a reader held the
lease: `Busy`. With the production lease adapter (macOS arm64, `--ignored`): `recover` while a
`Store` reader is open returns `Busy`; after the reader is dropped `recover`
succeeds and a following read returns the data; 200 `recover` calls against a
concurrent reader loop produced only `Busy` or complete data (recover done 1 to
4, busy 196 to 199; reader ok 2 to 4, busy 139 to 222), never another error or
partial data. **Found:** the platform adapter try-locks one process-wide state
mutex, so concurrent lease acquisitions in one process return `Busy` even for
unrelated roots (two such tests failed when run in parallel). That does not
affect separate ctl processes, but any future in-process concurrent reader must
expect `state_busy` and serialize. The recover tests with the production lease
are in-process; S6 must cover the cross-process case with the release binary.


# Part 1. Implemented at E1 (slice S2)

S2 scope: the read session and the `state-inspect` `-wal` row.
`Store::open`, `recover`, `probe`, `probe_with_scratch`, `open_with_scratch` and the
capped copy were untouched at E1 (E2-A slice A1a replaced them for open, recover and probe); `RecoveryReadSession`
also keeps its private-copy cap and `io.lock`. It is distinct from public
`recovery-activity`, which now uses `ReadSession` through the ctl snapshot adapter.

## Read session, exact requirements

1. Capture the binding (symlink walk, regular-file checks, identities). Take
   `store.lock` shared; lease acquisition is capped at 2 s. Never take `io.lock`;
   never write main or WAL content; no scratch.
2. Pre-checks, no SQLite: marker; header (application id, schema); empty
   rollback journal (nonempty refuses); **`-wal` exists and is a regular file**
   (`lstat`), else refuse. A missing `-wal` is not a legitimate engine state: all
   engine closes suppress checkpoint-on-close and TRUNCATE leaves a 0-byte file,
   and opening such a root would create an empty `-wal` and report an empty
   history (experiment f1). A race between this check and the open is
   acknowledged: a `-wal` deleted in that window gives the silent empty result,
   as damage does today. Then the lost-WAL acceptance record `store.wal-acceptance`
   (Decision B; format and states in storage-protocol.md): absent or `accepted` for
   the marker's generation proceeds, `pending` or `failed` refuses as the missing
   `-wal` row, and a foreign generation, malformed or oversized content or a
   non-regular file is an invalid marker. Refusal order: marker, header, journal,
   `-wal`, acceptance record, then later checks.
3. `SQLITE_OPEN_READ_ONLY`, checkpoint-on-close disabled, identity query against
   the marker, then the existing queries inside one read transaction.
4. `-shm` may be created or rewritten by a reader. It is derived state, rebuilt
   by any opener and never trusted when stale. Real sequences reach its absence:
   a copy that omits it, fixtures, and a non-engine client that closes last (which
   also removes `-wal`, so that case hits the `-wal` refusal, not just the `-shm`
   creation; such clients are outside the protocol per storage-protocol.md:18-19).
   If the directory is read-only and creation is needed, CANTOPEN or READONLY is a
   typed refusal, never an empty result.
5. Any SQLite error is a refusal.

### Deadline is soft

SQLite open and WAL recovery are uninterruptible, so the deadline bounds
**acceptance** of an answer, not elapsed time, I/O or lease occupancy: a read
that overruns is refused after the fact as `state_busy` and counted as a failed
request (R01). Values: 5 s for a read, 2 s for lease acquisition. The deadline
does not "bound cost".

### No hard WAL limit on reads

No size limit applies to the read session. With a live connection a 750 MiB WAL
costs about 1 ms; WAL length is not the count of valid frames; a 129 MiB WAL costs
about 27 ms and is correct; refusing it would count as a failed request under
R01; memory is small (a 750 MiB WAL has a 1.5 MiB `-shm`). WAL size is a health
signal recorded in every budget report; a cold read over a very large WAL can
miss its budget (161 ms sequential, 176 to 180 ms p95 for concurrent readers at
750 MiB) and is reported as a miss. A persistent overrun is not healed by retry:
its healer is a checkpoint by a writer role, which exists at E2. At E1 it is
unreachable except by fixtures.

## Wire codes (no new ones)

| situation | code | reason / message |
| --- | --- | --- |
| `-wal` missing, or not a regular file | `state_incompatible` | `infrastructure_unavailable`; message `write-ahead log missing; the history cannot be trusted` |
| CANTOPEN or READONLY (directory or file not openable), unreadable root, lease `Invalid` (lock not a regular file, root replaced under the lease) | `state_incompatible` | `infrastructure_unavailable`, message names the file; a failure not attributable to one file (the root, a sidecar type check) names the state root |
| deadline exceeded, lease busy, SQLite busy | `state_busy` | |
| SQLite corruption error (`malformed`, `not a database`) | `state_incompatible` | `infrastructure_unavailable` with the SQLite message |
| marker mismatch or SQL identity mismatch (generation differs) | `state_incompatible` | an existing P5 reason (`invalid_marker`) |
| database application id not ours; marker names an older schema; marker or header names a newer schema | `state_incompatible` | `foreign_store`; `unsupported_layout`; `newer_store` (as state-inspect reports them; a malformed marker is `invalid_marker` whatever its version) |
| nonempty rollback journal | `state_incompatible` | `infrastructure_unavailable` |
| `Error::Replaced` (identity changed during the read) | `state_incompatible` | `infrastructure_unavailable`; retry is allowed |
| default: any other SQLite error | `state_incompatible` | `infrastructure_unavailable` |

No code or reason is added; the implementation confirms each reason against the
existing P5 table.

## `state-inspect` row (an S1 amendment landing with the read path)

The inspector stays lease-free, scratch-free and SQLite-free but can `lstat`.
The row applies **only where the verdict would otherwise be `ready`**: precedence
for `newer_store`, `foreign_store`, `invalid_marker` and the journal rows is
unchanged, and an empty root stays `ready`. For a root with a valid current
marker and no `-wal`, or a non-regular `-wal`, the verdict is `incompatible` /
`infrastructure_unavailable` (`state_incompatible`) with the fixed message
`State root cannot be read`, never `ready` (`ready` must not describe a root
every read refuses). The same holds for the acceptance record: a bounded read of
`store.wal-acceptance` (after the `-wal` row) gives `incompatible` /
`infrastructure_unavailable` for `pending`, `failed` or an unreadable record, and
`invalid_marker` for a foreign generation, malformed or oversized content or a
non-regular file. `facts.entries` lists names, but `entries_truncated` at 256
cannot prove absence, so when it is truncated the observer checks `-wal` directly.
`ready` still does not promise that reads succeed: it also cannot see
main-file damage, and a read-only directory with no `-shm` or a read-only
`store.lock` (the lease opens it read-write) refuse too. Caller-gate row: markers
are read deny-side first; lineage reads go through the read session before any
task data. Where the P5 state answer precedes the gate, a missing `-wal` is
`state_incompatible`; where lineage itself cannot read a current-schema store, the
P7 row stands: `caller_undecidable`. No row grants anything beyond the reads.

## Healers: each refusal, E1 actor and E2 actor

| refusal | E1 actor | E2 actor |
| --- | --- | --- |
| `-wal` missing (including a non-engine client closing last) | none; unreachable at E1 except by fixtures or out-of-protocol clients; the seed tool never leaves a root without `-wal` | maintenance actor: contract `state-recover` archiving, then explicit accept-lost-WAL recover (Part 2); backup/restore must emit an empty `-wal` |
| cannot create `-shm` (read-only directory), read-only `store.lock` | operator fixes permissions | same; any writer-role open also creates `-shm` |
| `state_busy` (lease, deadline, SQLite busy) | retry; exclusive holders at E1 are `initialize_fixture` and `recover`, both short and test-only | retry |
| persistent deadline overrun (very large WAL) | none; unreachable except by fixtures | a checkpoint by a writer role |
| nonempty rollback journal | none (WAL-only primitive, unchanged) | unchanged |
| marker, identity mismatch, `Replaced` | none | contract `state-recover` |
| SQLite corruption error | none; there is no backup operation in Rust | contract `state-recover` archiving; the public remedy |

## Applied storage-protocol.md amendments (E1, read session only)

- The "Read session" paragraph records shared `store.lock` (2 s acquisition), no
  `io.lock`, `-wal` regular-file precondition, read-only open, soft 5 s deadline,
  refusals mapped as above.
- The `io.lock` hierarchy records that read sessions do not take it;
  the sentence about copying and opening source SQLite stays for the paths that do.
- The source-write boundary records that a read session may create or rewrite
  `-shm`, never main or WAL content, never `-wal`. The probe sentence is unchanged.
- At E1, probe/copy qualification, recovery ordering and the 64 MiB cap were unchanged
  for open/recover/probe and the internal private-copy `RecoveryReadSession`.

## E1 guarantee ledger (read session only)

| storage-protocol guarantee | effect for the read session |
| --- | --- |
| Probe never opens the source with SQLite | Not applicable to probe (unchanged); the read session opens the real file read-only |
| No sidecar creation or source writes by reads | Changed for the read session: may create or rewrite `-shm`; never main, WAL or `-wal` (missing is refused first, race acknowledged) |
| 64 MiB cap | Does not apply to the read session; applied to open, recover, probe and `RecoveryReadSession` at E1; since E2-A slice A1a only `RecoveryReadSession` keeps it |
| `quick_check` at open | The read session does not run it. Changed since E2-A slice A1a: `open` runs identity checks only (Decision A2); `recover` and `probe` scan in place |
| Lock hierarchy; `io.lock` | Read session skips `io.lock` (0 anomalies in the historical 3.49-million-read experiment, not a general safety qualification); mutators unchanged |
| No checkpoint on close or by readers | Preserved; a read-only handle cannot checkpoint |
| No immutable shortcut, no custom WAL parser | Preserved; `immutable=1` drops the WAL silently |
| Nonempty journal refuses | Preserved |
| Kill leaves scratch remnants | Improved for the read session (none; the existing path left one in 21 to 28 of 30); a reader that created `-shm` and is killed leaves it, confirmed |
| Readers do not disturb the single writer | Read session: 0 writer failures in 3 x 4 windows; existing path: 95 to 98 percent |
| Damage a read does not touch | Not detected by the read session (the E1 path's open-time `quick_check` would have; since E2-A slice A1a `open` does not run it either, only `recover` and `probe` do); verify is specified for E2, not implemented here |
| Payload damage | Returned as data (7 of 20 flipped-byte trials) by the read session; `quick_check` does not detect it on either path; an owner decision (open question 3) |
| Silent history loss on WAL damage | Not prevented; unchanged; owner accepted this only for E1 (D20). Watermark/generation sequence/recovery signal remain E2 work, not an E1-exit acceptance |

## Seed tool and E1

Fixtures are not forced checkpointed: the nonempty-WAL rebuild is the real E1
path. The seed tool pins no checkpoint on close and never leaves a root without
`-wal` (an empty file is fine). `authoritative_digest` excludes `-shm`. S6 measured
the release binary with the production lease adapter on a nonempty-WAL store, a
checkpointed one and a tail state, with 4 concurrent cold readers in the recorded
contention cases, and covered cross-process lease behaviour in
`hf-storage/tests/read_session_process.rs` and `hf-host/tests/read_session_production.rs`.
The figures are in the [E1 receipt](migration-receipts/E1.md); the contention cases are
recorded, not gated.

## Implementation note

`open_source` and `configure` cannot be the read-only path (they open read-write
and issue `PRAGMA journal_mode=WAL`, `synchronous` and `foreign_keys` writes), so
the read session is a distinct type.

# Part 2. Specified for E2, not implemented

Nothing in this part changes any E1 path. It specifies the replacement of the
64 MiB limit, as E1's exit requires, and lists decisions it does not settle.

## Replacement of the 64 MiB limit (writer open and recover)

Implemented in E2-A slice A1a for open, recover and probe: `open` runs the identity
checks only (decision A2), `recover` and `probe` add one `quick_check` bounded by the
deadline through an interrupt watchdog, `probe` without `io.lock`.

No size cap on any of them. Qualification runs in place through a read-only handle
(header, marker, identity, `quick_check`) instead of a copy; recover performs it
under exclusive `store.lock` and `io.lock` before mutation, once, and does not
scan again afterwards (an explicit verify is a separate request). Cost is about
0.46 ms per MiB of main file plus WAL recovery at 0.21 ms per MiB. This also
replaces the invariant at storage-protocol.md:47-48 without a copy.

## Decision A (owner): admission rule

Choose one; this goes to the owner.

| option | rule | cost |
| --- | --- | --- |
| A1 | full in-place qualification (`quick_check`) before every owner mutation, as today but without the copy | preserves "qualify before writable access" at its present strength (the read-only handle touches `-shm`); costs 184 ms at 397 MiB and about 0.9 s at 2 GiB per launch against launch p95 below 250 ms: exceeded above roughly 500 MiB of main file |
| A2 | owner admission checks header, marker and identity only; `quick_check` runs on a cadence off the launch path | launch cost independent of store size; **the rule "qualify before writable access" is weakened**: an owner may mutate a store not scanned since the last cadence run, so damage can be found after later writes |

The ledger does not claim that qualification before writable access is preserved
under A2.

## Decision B: lost WAL and writer-role openers

Every writer-role opener (owner, recover, seed) prechecks `-wal` exactly as the
read session does. Accepting a lost WAL is explicit and only in maintenance.
Ordering problem: in-place read-only qualification itself creates an empty `-wal`
(f1), so a failed qualification could leave an apparently legitimate root.
Specified resolution to settle: the maintenance actor first writes a durable,
generation-bound acceptance record in state `pending`, then qualifies, then marks
it `accepted` on success or `failed` on failure. Readers and openers treat
`pending` and `failed` as the missing-`-wal` refusal even when an empty `-wal`
now exists. Acceptance becomes durable only at `accepted`. The record format, reader
and precheck hook are implemented (A1b); the maintenance writer arrives in E2-B.

## Maintenance actor

`recover` runs from a maintenance actor when no owner is alive and exclusive root
ownership is available after shared leases quiesce (contracts.md:74). The earlier
flow "owner drops its handle, calls recover, reopens" is deleted: it contradicts
that contract line.

## Checkpoint owner

The writer role owns checkpointing. Specified policy: `journal_size_limit` 16 MiB
and a PASSIVE checkpoint after a commit once the WAL passes 8 MiB. Measured: a
passive checkpoint does not shrink the file; with the limit it shrinks at the next
write after a complete checkpoint. RESTART is not part of the design (no
experiment ran it). A commit-independent open-time checkpoint step and a shrink
path for a WAL pinned by readers are E2 items, as are open-time checkpoint cost
and `io.lock` `Busy` behaviour among several per-task owners: the safety evidence
here is single-writer. Starvation by overlapping readers: reads are short
deadline-bounded transactions; under four saturating readers 1,078 of 1,083 passive
checkpoints completed; a pinned mark grows the WAL, reads continue, and WAL size
shows in the health signal.

## Verification cadence and a failed check

`quick_check` runs on explicit verify, once inside recover before mutation, and at
a recorded per-generation cadence (for example once per generation per 24 hours,
due time in a small generation-bound file, reset after a successful recover),
started off the launch path by the first owner that finds it due, on a read-only
snapshot without `io.lock`, so it cannot make sibling commits fail `Busy`; a long
snapshot only delays checkpoints. Launch p95 below 250 ms excludes it; its cost is
a background cost to be measured at E2 against write p99 below 20 ms.

Outcome of a failed cadence check, to settle at E2: the verifier writes a
generation-bound fence in state `failed`; readers refuse (`state_incompatible`,
`infrastructure_unavailable`) and new owners are not admitted. A running owner
treats the fence like an ambiguous storage write in owner-output-drain.md: it
performs no further storage mutation and no retry, keeps its reader draining
(that reader performs no storage I/O), keeps its Store/root lease, and publishes
no terminal state that requires a storage commit; reservations persist, with no
release, replacement or adoption. Reconciling the fence's timing with the
drain's terminal publication is part of the E2 decision.

## WAL damage and the commit watermark (E2-exit decision)

A damaged, truncated or lost WAL yields a consistent but wrong or empty
**successful** history, including `Ok(0)` for a task's events, and SQLite cannot
detect it: frame checksums end the log at the first bad frame, and a torn tail
after a killed writer is indistinguishable from damage. This is unchanged from
the capped path. E1 has no public writer; owner acceptance of silent WAL rollback
is limited to E1 (D20), not a claim that damaged fixtures cannot encounter it.
The watermark/generation sequence/recovery signal remain named E2 carryover.
Unmeasured: with a live owner whose wal-index is trusted, damaged frames behind a
valid index may return wrong rows rather than rolling back.

Watermark sketch:

- Evidence: a small file bound to the store generation holding the last
  acknowledged commit sequence; the sequence is also a row incremented inside
  every write transaction.
- Writers serialize commit and publication (under `io.lock`): commit sequence n,
  then make the file durable at n, then acknowledge. Invariant: file <= database.
  A crash between commit and file write leaves the database ahead, which is
  legal; file-first ordering would flag legitimate crashes and is rejected.
- A reader reads the file **before** opening its snapshot, then reads the
  database sequence in the snapshot, so a concurrent commit can only make the
  database ahead. Database below file means acknowledged commits were lost:
  refuse. Unacknowledged commits cannot be detected.
- Generation binding: the file carries the generation; mismatch refuses.
- Restore of an older backup trips the check by design; maintenance accepts a
  rollback and rewrites the file.
- Cost: one extra durable small write per acknowledged commit against write p99
  below 20 ms. A commit is about 0.09 ms here with plain fsync, but macOS `fsync`
  is not a full flush; the `F_FULLFSYNC` cost is unmeasured and decides it.

## E2 ledger notes

| storage-protocol guarantee | E2 effect |
| --- | --- |
| 64 MiB cap | Removed for writer open, recover and probe by in-place qualification (implemented, E2-A slice A1a); kept only for `RecoveryReadSession` |
| Qualify before writable access | Decision A1 preserves it; A2 weakens it; not claimed otherwise |
| `quick_check` at every open | A1 keeps it per launch; A2 moves it to cadence, verify and recover |
| Lost WAL | Refused by every writer-role opener; acceptance only in maintenance, per Decision B |
| Silent WAL damage | Unchanged; watermark decision at E2 exit |

# Budget verdict (read session)

With the WAL kept small or a live connection, cold operation p95 is 0.6 to 3.2 ms
on both datasets against 50 and 100 ms, about 6.2 ms end-to-end. On the CI `wal`
store, sequential: 10 to 12 ms (status 10.7 ms); with 4 concurrent readers and no
holder it is bimodal (only 201 to 207 of 1,000 under 5 ms, the rest about 11.5
ms). At a cold 750 MiB WAL it is 161 ms sequential and 176 to 180 ms p95 for
concurrent readers, missing both budgets, reported as a miss with WAL size as the
health signal. The existing path misses status on the CI `wal` store (72 to 113
ms) and cannot serve the large store. Caveats: candidate queries are
representative SQL; the file cache was not evicted; the lease adapter was the
test one; budgets are gated on this machine; r2, r3 and c1 to c3 are the
unflagged repetitions relied on.

# Open questions

1. Decision A (owner): A1 or A2 for owner admission, and what A2's weaker rule
   costs the owner.
2. Unchanged silent WAL rollback is owner-accepted only for E1 (D20). Decide the
   watermark/generation sequence/recovery signal at E2 and settle Decision B's
   acceptance ordering.
3. Payload damage: single flipped bytes are returned as data (7 of 20 trials) and
   no SQLite mechanism available here (no page checksums in the bundled build)
   detects it on either path. Accept as unchanged, or require a checksum
   mechanism?
4. Fence semantics and reconciliation with owner-output-drain.md for a failed
   cadence check.
5. Cadence period and where its due time lives; owner thresholds (8 MiB, 16 MiB
   limit) are CI-scale and need S6 confirmation.

# Historical measurement exclusions and remaining qualification

The S2 measurements did not cover cold file cache (needs privileges), the production
lease adapter across processes (the recover tests were in-process), or production
queries on a read-only handle. Those queries are now implemented and the release-binary
budget runs are recorded in the [E1 receipt](migration-receipts/E1.md). Other unmeasured items:
a live owner with a write transaction; damaged frames behind
a trusted wal-index; `F_FULLFSYNC` cost; open-time checkpoint cost and multi-owner
`io.lock` behaviour; RESTART checkpoint behaviour; the stress dataset, workflows
and runs; power loss; other platforms. Payload corruption and the large-integer
issue remain open. Part 2 is an E2 specification, not implemented behavior or
acceptance of its proposed policies.

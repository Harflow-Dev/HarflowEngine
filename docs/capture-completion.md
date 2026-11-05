# Private durable capture and completion (R03-B2c)

This foundation extends the fixed private fixture owner handoff. It is not a
supported engine runtime, backend, task lifecycle or public capture API. The
scaffold's default invocation remains unsupported. HarflowMCP retains canonical
contract ownership; the selected full207 public fixtures are unchanged.

## Boundaries

Only the existing fresh single-threaded parent/owner/effect modes participate.
They retain empty environment, qualified piped stdio and descriptor admission.
All Store and capture handles are closed before any child spawn. No Store handle
spans a protocol, pipe, process or fixture-gate wait. Capture begins after the exact
EffectDone frame; protocol bytes are never raw output. The fixed child then emits
a deterministic nonsecret binary suffix on stdout and stderr. No arbitrary command,
prepared payload or credential is used as captured output.

Acceptance and completion are separate. The owner records acceptance before
capture completion, retains its original child as sole waiter, and completes capture
before sending the final Accepted frame. Parent loss or acknowledgment failure does
not grant replay permission. Existing ownership intent/reservation remains active;
completion never calls ownership_release or invents owner-death/containment proof.
Containment remains Unknown. No restart discovery, adoption or replacement is added.

## Fresh schema and private evidence

Fresh isolated roots use schema4. Earlier schema3 and older roots refuse without
upgrade, initialization, normalization or repair. The existing marker protocol stays
version2 with the actual current schema in its schema field. Prior receipts retain
the schema selected by their historical delivery.

Capture binds the exact operation, generation, attempt, owner ID/nonce/process,
current acceptance fence, original child identity and distinct raw file identities.
Capture progress has a separate compare-and-swap revision; it does not advance
ownership revision or modify task headers/watch revisions. Every mutation rechecks
the current ownership fence and child. Cancellation or a changed fence refuses.

Offset advancement and its private capture event commit in one immediate SQLite
transaction under the existing FULL/WAL/binding policy. These events are internal
capture evidence, not canonical task-events or a task status/summary projection.
Reads preflight bounded scalar types and sizes before decoding. Capture metadata
is a trusted internal observation seam, not proof against a caller fabricating
filesystem/OS facts. The host supplies offsets only from successfully synced raw
extents and exit only from the retained child's actual wait result.

## Raw publication ordering

The fixed operation-derived stdout/stderr files are created exclusively under the
isolated root's parent after child spawn. Creation refuses existing files; no append
recovery, truncate, reopen or repair is implemented. Each file must be a regular
single-link file with the expected device/inode. The writer uses nofollow,
nonblocking open and an exclusive nonblocking file lock. The parent directory is
synced before zero-offset metadata is published.

For every chunk the owner follows this order:

1. Take bounded new bytes from each nonblocking stream, preserving independent
   EOF, overflow and error evidence.
2. Check expected file lengths and path/open-file identities, append bytes, sync
   both raw files, recheck identities and the original deadline.
3. Publish the returned synced offsets and EOF/loss evidence with the exact capture
   revision; offsets and private event become visible together.
4. Once every taken byte is published, both streams reached EOF and the retained
   child has a real exit result, publish terminal completion and its event atomically.

Partial append, failed sync, late raw-sync return, source replacement, stale state
or an uncertain commit stops the operation. A SQLite commit already admitted may
complete after its deadline; it retains the existing committed-or-unknown outcome
semantics and cannot be undone or used to authorize a retry. Raw bytes may exist beyond indexed offsets;
those bytes are not claimed durable/indexed completion and are never retried here.
Observers do not advance offsets, finish a writer, repair tails or release a claim.

## Bounds and terminal truth

Each raw stream has a65536byte total limit and each append accepts at most16384bytes
per stream. Pumping divides its fixed read budget between the two streams. Taking
bytes clears only the buffered bytes; EOF/error/overflow remain separate. There is
no background queue or worker thread. A slow storage command stops pumping and
applies ordinary pipe backpressure under the same absolute fixture deadline.

Private events are capped at128 per operation including terminal publication.
Highly fragmented output may reach this limit before the raw byte limit; the byte
limit is a maximum, not a promise that every fragmentation is admitted. No-op
progress, reversed offsets, writes after a published EOF, stale revisions and changes
to the binding refuse. All scalar and revision conversions are checked.

A terminal row requires published EOF on both streams, no known loss, and an observed
exit code or positive terminating signal. Immediately before terminal publication,
the host rechecks both raw path/open-file identities and published lengths after
the pre-terminal gate. These cooperative checks are not an atomic defense against
a hostile concurrent filesystem writer. Exit alone is not drain completion; EOF
alone is not child exit. Overflow, pipe error, deadline or publication failure remains
incomplete and cannot produce lossless terminal completion. A terminal row is immutable.
There is no public success/summary mapping in this slice.

The existing10/8/4second parent/owner/effect protocol budgets and bounded qualified
cleanup remain. Filesystem/SQLite/kernel calls are synchronous and nonpreemptible;
admission and observation deadlines do not guarantee a hard wall-clock bound on
arbitrary filesystem stalls. No RSS, throughput, p99, production scale or arbitrary
agent/descendant qualification is claimed.

## Qualification and remaining gates

See [the B2c receipt](migration-receipts/R03-B2c.md) for observed tests and review.
Deterministic gates expose append-before-sync, sync-before-publication,
post-publication and pre/post-terminal boundaries. Independent raw file bytes,
read-only SQLite and the external effect ledger are the fixture oracles.

R04 remains responsible for restart/owner-death/adoption decisions, reboot/PID reuse,
real disk-full/power/I/O faults and broader recovery. Public runtime/authority,
arbitrary backends, task/event/status mapping, descendant containment, owner
longevity and whole R03/R14 acceptance remain outstanding. No runtime selection,
installation, registration, sibling application or public contract changed.

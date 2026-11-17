# E2-C2 — Internal task preparation and transaction spine

Status: unqualified source checkpoint, 2026-11-17. Public run/cancel and private
production process routes remain closed. This is not first-task, E2 or release
acceptance; no existing client selects this engine.

The checkpoint adds normal PATH/version/session/stdin preparation, private T0
intent storage, atomic observed-child T2 publication, separate full-control
acceptance, typed terminal publication and positive pre-agent refusal. Schema 8
adds lifecycle evidence, private intent and acceptance tables; existing roots are
not upgraded. Task projection read/write capacity is 16 KiB with pre-allocation
budget checks; both historical fixture writers retain their 2 KiB limits.

Platform adapters add neutral inherited-stdio and direct-parent qualification,
owner session entry, retained channels, production reader bounds and prepared raw
capture. Host composition retains the original root, child and writer, validates
immutable preparation and actual ancestry, and permanently poisons uncertain
mutations. Read/discard remains available independently on both writer streams.
Cached readiness is historical evidence; channel drop does not prove stdio EOF.

The caller inventory distinguishes legitimate ordinary parents from recorded
dispatchers, owners and agent children. Exact preparation exclusion removes only
one fully matching Prepared row. Malformed, unknown and over-budget evidence
refuses. Socket actor generation is still unqualified; no socket mutation is
admitted and no additional dependency was adopted.

## Observed verification

- New core focused tests: 9 interfaces and 21 storage definitions passed. Separate
  lifecycle/schema/backend preparation tests: 30 passed. The poisoned-channel
  discard regression also passed.
- OS-enabled workspace run: every target passed except one storage-library
  snapshot test returning Busy during initial fixture publication. Its focused
  rerun passed, followed by the entire 197-definition storage library with its
  normal two-thread concurrency. This is not a claim of one clean full-suite run.
  The earlier sandboxed run could not qualify process identities or local sockets
  and is retained as failed environment evidence.
- Current workspace check, release build, formatting, strict workspace/all-target
  Clippy, architecture/legacy guards and diff checks passed.
- Core review closed at round 3; platform bootstrap and host/discard reviews closed
  at round 2. A legacy fixture-cap regression found by the full tests was restored;
  its existing unchanged fixture tests and production 16 KiB boundary tests passed
  in the post-fix OS-enabled run.

Full Python compatibility checks, bidirectional old/new binary refusals, strict
frozen-seed logical/physical parity and actual production OS execution remain
open. An external old-seed witness smoke captured 2 checkpoints successfully;
complete witnesses do not themselves establish physical size equivalence.

## Next product checkpoint

The owner approved run/status/observed completion before caller-authenticated
cancellation. Canonical candidate semantics are pinned to HarflowMCP
`4cb9d3270bbe54ee9fcafd287617cb6f4e1c9bd0`, owner-runtime manifest
`d893c94a7f3406ed9565193104f46359b43b35a63a9e989e63ea9b2a37400a90`.
Qualification executes only the pinned PATH fake; no real provider, adoption or
capability promotion is authorized.

Actual T2 publication plus retained raw/reader/owner/writer readiness may earn a
distinct private Started reply and the existing public task-id acknowledgment.
Full-control AcceptanceRecorded stays unwritten until operational control is
qualified. Cancel must refuse before lookup, mutation or signaling. The first
backend prompt limit remains 1 MiB, enforced before T0; 4 MiB intent capacity does not
broaden backend admission. The dispatcher, owner/writer command loop, actual
capture/event/terminal integration and normal release-binary fake oracles remain
to be implemented and verified.

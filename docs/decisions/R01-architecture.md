# R01 architecture decisions

2026-11-04. Architecture specification only; no implementation or empirical proof.
See [receipt](../migration-receipts/R01.md) and [criterion matrix](R01-validation.md).

## Authority and baseline

M01's subsequent limited owner decision accepts SB1-A specification/fixture input
and parallel M02/R01 entry. It does not accept SB1-B/C/D, aggregate conformance,
all historical receipts, the Proposed 47/eight MCP partition, native budgets,
Python 0.0.8, or a shipping runtime. R02 requires separate assignment.

Pinned canonical source: HarflowMCP b0185be855fd13acc94cb2d2008400d285d15df2.
S05 fixture producer: b69b5754a7b6496aaf5e3106116e302ea7a10675.
Complete 199-fixture manifest SHA-256:
383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0.
Separate 55-tool MCP snapshot SHA-256:
8d64130f2fc8ff2827cb5b4c1f8b737a9c917bf218ef5e0af14fb2c5f9bd5478.
These identities establish input provenance, not installed or artifact qualification.
Canonical contracts remain MCP-owned until accepted O04. Tracker owns the single
delta queue: producer spec/capability/fixtures/tests and immutable commit first,
then exact Swift/Rust adoption. Missing API never permits private reads or app MCP.

Accepted inherited constraints: separate engine process; fresh bundled SQLite
root; no shared Python writer root, history import, live-process adoption or
unknown mutation replay; durable operation/reservation identity; exact-action
responder credentials outside agent-readable roots. Explicit allocation coordinates
exclusive resources across roots. M12 and R14 both gate I01.

## Layer and policy decisions

Routine engineering decisions below are selected design inputs within the assigned
scope, subject to review and subsequent proof. They do not accept affected behavior.
Domain owns pure values/transitions; application owns operation-shaped ports;
storage/backends/platform own concrete effects; interfaces maps transports; host
composes. Keep constructor injection, explicit errors and bounded work. Preserve
250/50 targets and 400/80 maxima, unsafe prohibition and production unwrap/expect/
panic rules. The existing dependency/domain guards remain unchanged.

hf-contracts remains empty now. A later explicit policy change may give it consumer
wire types validated against the pinned complete bundle; this never transfers
canonical ownership. No generated consumer schema becomes a second specification.

## Ownership, identity and containment

Select dedicated detached task owners and a separate workflow scheduling owner,
not a mandatory central service. Launch a dedicated owner executable mode through
a bounded private inherited channel; do not copy Python fork implementation by
habit. Host owns composition; platform owns spawn, handles, identity and signals;
application owns intent/claim transitions. A central service is an alternative
only after an owner-approved lifecycle/availability change and independent proof.

Protocol: commit intent/reservation before spawn; pass prepared request, digest,
root/store generation and claim identity privately; owner validates and atomically
claims; persist observed spawn identity before reporting acceptance. Reservation
is exclusion evidence, not executable authority. Lost acknowledgment can mean an
effect occurred: observe/reconcile without relaunch. Only proven non-execution
permits an explicit retry under accepted semantics. Workflow replacement preserves
issued attempts and never treats missing observations as proof no worker ran.

Process identity is PID plus high-resolution start identity and boot generation,
with matching/dead/unknown verdicts. Retain direct child handles where possible.
Cross-process signals require freshly sufficient identity; unknown refuses control
and release. Never reuse saved non-child PID/PGID blindly, including after reaping.
Do not add Python legacy-marker fallbacks to fresh Rust roots.

Select POSIX session/group control as the initial macOS mechanism, not a claim of
arbitrary descendant containment. Drain both streams concurrently with bounded
buffers; request cancellation, escalate only against qualified owned targets,
wait boundedly and publish exit/capture/drain separately. Detached/session-escaping
children remain a proof obligation. If complete containment cannot be established,
report incomplete/unknown cleanup and retain reservations; an owner decision is
required before narrowing the promised guarantee. Later Linux/Windows adapters
need independent qualification; scaffold CI does not qualify process behavior.

Required follow-up: launch/ack race barriers, observer death, owner death, reused
PID and boot changes, escaped trees, pipe holders and signal races. No spike ran.

## Store, contention, capture and backup

Select bundled SQLite, local-disk WAL, FULL synchronization and foreign keys on
every connection as the initial design to validate. Refuse unsupported storage
locations rather than assume network-filesystem semantics. Each process uses a
bounded storage execution queue; transactions never wait for process/network/
stream/user work. No global writer service. Queue admission and transaction busy
handling have explicit finite deadlines; retry only known uncommitted database
work, never an external mutation whose outcome may be committed. Exhaustion maps
to an accepted refusal/unavailable result, never empty success. If canonical wire
semantics are insufficient, queue a producer-first delta before exposure.

Use typed indexed identity/state/revision/filter columns and validated bounded
variable payloads. Unique constraints plus compare-and-swap protect session and
claim transitions. A lease timeout alone does not prove an external owner dead.
All writers qualify store generation/schema before writing; exclusive restart-safe
schema changes refuse future schemas. Read snapshots are bounded in rows/bytes/
duration; opaque cursors bind query/authority/generation, never SQLite row offsets
that accidentally survive incompatible replacement. Public cursor semantics stay
canonical even though Python file identity is not the Rust storage implementation.

One designated checkpoint claimant per store generation performs bounded passive
work; long readers cannot trigger unbounded foreground checkpoint waits. WAL growth
limits, admission deadlines and batching sizes must be chosen by measured R02
contention work, not asserted here. Escalating checkpoint modes require a bounded
maintenance window. No threshold may be weakened to meet a budget.

Normalized events have one authoritative SQLite representation. Raw vendor captures
remain managed files. Ordering: append captured bytes; establish the declared
flush/sync boundary; commit qualified offsets/events; only then acknowledge durable
facts. A crash may leave an unreferenced tail, never a trusted offset beyond durable
bytes. Reconcile boundedly and expose missing/corrupt capture. Pipe draining must
not block behind SQLite: bounded spool/backpressure failure becomes visible capture
loss or controlled stop, not success. Batching must not weaken accepted-message or
terminal-result acknowledgment durability. Terminal exit and fully drained capture
remain independent facts.

Use SQLite backup API or quiesced/checkpointed snapshot, plus a consistency manifest
for engine-owned files and qualified offsets. Verify restore into a new isolated
root. Never copy only an active WAL main file or traverse/copy external artifact
references implicitly. No old histories are imported. Power-loss guarantees require
an explicit filesystem/device fault model; process-kill tests prove less.

## Operation identity and external effects

Persist operation ID, payload digest, reservation ID, attempt and store generation
before effects. Reuse of an ID with a changed payload refuses. Intent, claim,
observed effect and settled result are distinct transitions. A database commit
cannot atomically include spawn/provider delivery: after possible effect, absence
of acknowledgment means unknown, not retryable failure. Reconciliation observes
stable identities without creating a new attempt. Internal IDs add no public
idempotent retry guarantee. Any new public reconciliation operation or retry rule
requires the canonical MCP delta process and explicit semantics approval.

## Responder authority: specified proposal, not accepted new public API

Recommend engine-issued random narrow responder credentials; native Keychain holds
the client secret outside agent-readable roots. Engine verification material also
needs an OS-protected credential facility, not plaintext SQLite. Define a credential
store port, separate from ordinary persistence. Agent children receive neither
secret nor signing endpoint access. Lineage is deny-side evidence only.

Recommend enrollment by explicit human-mediated pairing of the selected immutable
engine, store generation and native client. Record credential ID/generation and
role. Rotation enrolls a replacement, confirms custody, then revokes the old key;
interrupted rotation remains observable and does not silently revive revoked keys.
Revocation disables new decisions without rewriting already durable decisions.
Lost/locked secure storage fails closed; recovery requires explicit re-enrollment.
No credentials are imported from Python or shared between roots.

Recommend authenticated ctl invocation with secret material on a private inherited
descriptor, never argv/environment/logs/state files. Bind proof to request nonce,
operation/store identity, task/request/action digest, current owner revision and
role; consume nonce and reserve exact decision atomically. Duplicate, expired,
revoked, wrong-store and stale decisions refuse. Decision reservation, delivery and
execution outcome remain separate; response loss triggers observation, not resend.
Persist audit provenance without secret material. Descriptor inheritance into agent
children must be explicitly prevented.

This is a proposed canonical transport/enrollment extension, not permission for
Rust-only flags or semantics. Owner must adjudicate pairing/recovery and threat
boundary; MCP lead specifies public semantics before R07 implementation. Same-UID
cooperative tests are not hostile-peer authentication. Broadly unsandboxed agents
may reach same-user resources: Keychain placement alone does not prove separation.
Qualify access control, caller identity, endpoint reachability and secret extraction
on the claimed platform; inability to enforce the boundary blocks authority release.
Alternative persistent authenticated local service adds endpoint/lifecycle attack
surface and is not selected without a concrete need and owner decision.

## Interfaces and minimum R02 footprint

Product API is ctl only; setup acquisition/registration is separate; MCP is agent
transport only. All map to shared application use cases with independent protocol
validation. Fresh handshake precedes each operation with the same runtime/root/
HOME/environment/cwd; validate each response independently. Preserve strict integer
versions, historical inner event provenance, capability negatives, scoped prefix
resolution, bounds and unknown/refusal precedence. Do not advertise capabilities
until the complete operation predicates are implemented. Partial development-engine
capability representation must be reconciled with MCP before a compatible claim.

R02-A: fresh store identity/schema and internal transaction primitives; pure envelope,
root and version-info mapping. Missing-root probes create nothing; incompatible
nonempty/Python roots refuse. Explicit store initialization is an internal fixture
operation until an accepted public creation path exists, not an invented ctl command.
R02-B: task headers/status/task-list-page against isolated Rust-owned fixtures,
including authority-before-prefix, all supported filters, pending launches, partial
ownership and query/generation cursors. Do not advertise a partial command.
R02-C: stored event pages with canonical filtering/order/bounds/capture/drain and
cursor invalidation. No fabricated live/final evidence or Python file-layout API.
Full fixture bundle is the adoption unit; operation subsets are implementation scope.

B10 authority and B11 bounds/history criteria gate public read qualification; they
are not deferred simply because reads do not spawn. Pure decoder/store development
can proceed on separate assignment while missing full behavioral proof is recorded.
No state-recover, successor capability, workflow/resource/statistics implementation,
launch/control, provider, setup mutation or credentials in the first store/read scope.
SB1-B/C/D and recovery scope remain unaccepted. If a listed read depends on an
unaccepted semantic, isolate its internal mechanics and stop that public adapter.

## Dependency and distribution decisions

Proposed dependency roles, not preapproval: serialization in contracts/interfaces
and bounded storage payloads; rusqlite bundled in storage; safe OS wrappers in
platform; reviewed crypto/secure-storage adapters only for responder work. Domain
gets no effectful/serialization macros by default. Standard blocking work plus
bounded dedicated threads is the initial store design; defer an async framework
until process/stream requirements justify it. Exact crates, versions, features,
transitive native libraries, license/security policy and guard edges need a narrow
reviewed implementation change. No Cargo or enforcement changes occur in R01.
Unsafe remains forbidden in workspace source; safe wrapper limitations are a blocker,
not permission for an undocumented exception. Dependency-internal unsafe requires
reviewed provenance and feature scope; it is not covered by the source lint.

Recommend independent immutable macOS arm64 artifacts, bundled SQLite, no Python,
ambient system SQLite, developer PATH or sibling-source dependency. Runtime closure
records compiler/target/build inputs, lockfile, native linkage/SQLite options,
artifact/source digests, licenses and signing provenance. Harness binaries/auth are
external prerequisites with separate exact-version qualification. Version-info is
compatibility evidence, not artifact authenticity.

Owner distribution choice remains open: recommend app-carried pinned artifact for
offline first acquisition, staged and explicitly selected by native manager.
Alternative separately acquired signed/digest-pinned artifact reduces app size but
needs an offline provisioning/recovery story. Both preserve immutable generations,
installation lock, integrity validation and independent app/engine releases. No
startup download, registration, root switch or rollback over incompatible new state
is implicit. Mac owns acquisition/secure storage integration; Engine owns artifact
closure. This choice blocks I02 packaging, not isolated R02 store work.

## Concrete owner decisions retained

1. Distribution: app-carried pinned artifact recommended; separately acquired
   alternative above. Decide before I02 packaging, no release authorized here.
2. Responder: explicit human pairing/re-enrollment and authenticated inherited-fd
   ctl transport recommended, subject to proven OS credential isolation and MCP
   canonical extension. Decide before R07/I01, not before basic R02 mechanics.
3. Any demonstrated containment limitation: preserve unknown/incomplete and block
   control qualification by default; only owner can change scope/guarantee.
4. Exact MCP/ctl scope and SB1-B/C/D remain with tracker/owner, not inferred here.

No broad new approval is needed for routine short-transaction, bounded-queue,
constructor-injection or port choices. R01 receipt delivery is distinct from owner
acceptance; empirical proof and every affected gate remain outstanding.

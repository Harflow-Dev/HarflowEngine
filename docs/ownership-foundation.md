# Internal durable ownership foundation (R03-B1)

This namespace is authoritative only for isolated internal intent/reservation/claim
transactions. It is separate from R02 task/event fixture DML. No production owner,
agent execution, public command, authentication or capture ingestion is implemented.
The scaffold remains unsupported. See the [receipt](migration-receipts/R03-B1.md).

## Immutable preparation and rights

A prepared request binds operation, session, reservation, attempt, store generation,
SHA-256 digest and at most16384 opaque payload bytes. IDs are bounded32hex values,
with a64hex digest; constructors normalize hexadecimal case. Storage verifies the
actual SHA-256 and compares exact bytes, so a reused digest cannot hide a changed
payload. These are internal prepared bytes, not an invented public JSON
canonicalization or retry protocol.

Prepare atomically persists the request and excludes another active reservation
for that session. Reservation and attempt IDs remain unique across history.
Identical operation/request replay returns Existing; any immutable change refuses.
Neither Prepared nor Existing grants an execution right. A released operation is
never reset or turned into a new attempt.

| Current phase | Permitted transition | Required condition / result |
| --- | --- | --- |
| absent | Prepared | New immutable request; unique active session/reservation/attempt. No execution grant. |
| Prepared | Claimed | Exact revision, not cancelled; one committed CAS returns Granted. |
| Claimed | SpawnObserved | Exact current owner fence; bind one supplied child identity. |
| SpawnObserved | AcceptanceRecorded | Exact fence; child identity already durable. |
| Prepared | Released | Cancellation requested, exact revision, explicit Unclaimed evidence. No claim grant ever committed. |
| Claimed | Released | Exact fence plus positive NeverStarted and owner-dead fixture evidence; no child observation. |
| SpawnObserved/AcceptanceRecorded | Released | Exact fence/child plus positive owner-dead, terminal exit, completed drain and containment fixture evidence. |

Every claim on a non-Prepared phase returns NoGrant, including a retry by the same
owner after response loss. Observing a snapshot or deriving its current fence does
not mint a right. A claim can commit and its response be lost: Claimed means an
effect is possible even when no spawn row/ledger/acknowledgment exists. Missing
facts never establish non-execution. There is no lease timeout, reclaim/reset,
transfer or automatic retry transition.

AcceptanceRecorded is an internal durable marker before a fixture acknowledgment.
It does not prove a peer received that acknowledgment, or publish a public result.
A caller must observe/reconcile after response loss, not execute again.

## Fences, cancellation and release evidence

A fence binds operation, store generation, attempt, owner ID, claim nonce, supplied
owner process identity and revision. Owner updates compare the entire fence in the
same transaction. No owner/spawn identity can be replaced. Repeating an unchanged
spawn/acceptance update with a fresh fence is observational; a stale fence refuses.

Cancellation is a separate sticky bit and increments revision exactly once. It
neither signals a process nor proves cleanup/non-execution. The owner must refresh
its observation after a cancellation race; refresh does not create a new grant.
A cancelled Prepared request cannot be claimed. Claimed/observed cancellation keeps
the reservation until explicit release proof succeeds. Releasing an old operation
cannot remove a newer operation's session reservation.

Release proof is committed atomically with Released. Its bounded private v1 audit
encoding stores evidence ID, operation/session/reservation/attempt/generation,
prior phase/revision/cancellation, owner/claim/process, applicable child and terminal
facts. Released observations reparse the audit and compare the complete bound
context. An evidence ID alone is not accepted. The encoding is private storage,
not a public wire contract, credential or authenticated certificate.

NeverStarted is explicit positive fixture evidence for the fence owner, including
owner death. It cannot be inferred from absent spawn metadata or a missing effect
ledger, and it contradicts an already observed child. Terminal release requires
that exact child, Code/Signal rather than Unobserved, owner death, completed pipe
drain and completed containment. Unknown/false required cleanup evidence refuses
and keeps the reservation. Capture loss/history completeness is a separate audit
fact: a known-loss or unknown-history capture can still be fully drained after
physical cleanup; release never rewrites that into lossless history.

All process/death/containment facts here are TRUSTED FIXTURE INPUTS. They do not
authenticate a nonchild process or qualify real cleanup. R03-A returns containment
Unknown and cannot supply these positive terminal release proofs. Claim nonces and
fences are database concurrency tokens, not secret responder credentials. Future
owner integration must validate authority/evidence before invoking these ports.

## Schema and transaction boundary

This suffix deliberately advances the fresh internal SQLite root schema from1to2.
The cooperative marker remains protocol harflow-storage-2, with its second line
encoding the actual schema2. Parsing never relabels a schema1 marker as2. Existing
schema1, future and mismatched header/marker roots refuse; no existing root is
upgraded, read-initialized, imported, reset or adopted. Only exclusive initialization
of a nonexistent fixture root creates the ownership namespace.

ownership_meta version1 binds the root generation. ownership_records holds the
immutable request and current state; a partial unique live-session index excludes
all phases except Released, and reservation/attempt uniqueness retains history.
Namespace metadata, bounded column types/lengths, complete owner/spawn groups,
phase invariants, strict booleans and reachable revisions are validated when
materialized. Stored payload SHA-256 is rechecked in the same snapshot before
returning observations or granting/transitions. Inconsistent rows fail closed;
these checks are not authentication of arbitrary raw writers or whole-database
semantic fault qualification.

Writes use existing admission: handle mutex, cooperative I/O gate, short SQLite
IMMEDIATE transaction, conditional revision UPDATE and uniqueness constraints.
They retain WAL/FULL/foreign keys, disabled automatic/close checkpoints, explicit
recovery and64MiB main+WAL qualification. No network/process/stream wait occurs in
production transactions. Test-only before/after-commit barriers are compiled only
for Unix unit tests; normal builds contain no stage environment control.

Known-expired pre-commit work rolls back/refuses without a queued late mutation.
After commit may have occurred, return the committed result (possibly late) or
CommitOutcomeUnknown; a later deadline, binding failure or missing acknowledgment
cannot become definite non-execution/no-grant. SQLite/OS calls cannot be forcibly
preempted inside a syscall. No public retry guarantee follows.

Observe performs bounded snapshot reads, not claim/lease/revision writes. SQLite
reader bookkeeping is not a promise of byte-identical SHM. All payload/scalar
materialization is bounded before Rust allocation and respects Store row/byte
limits. This is not a scale/performance qualification or a global scan that
certifies every unrelated stored row. Schema-changing raw writers bypassing the
cooperative protocol remain unsupported; R04's broader corruption/fault work is
not replaced by these checks.

## Dependency and handoff

Only storage adds exact sha2=0.10.9, default features disabled, no declared features.
[Upstream package](https://crates.io/crates/sha2/0.10.9) is MIT/Apache-2.0; the lock
pins its small digest/CPU-dispatch closure. No assembly feature, workspace unsafe
exception, domain dependency or public crypto/credential interface is added.
Hashing is bounded content-consistency work; it is not request authentication.

The [R03-B2 fixed fixture composition](owner-handoff.md) adds a bounded prepared
request/claim/observed-spawn/acknowledgment protocol with actual process identity.
It remains separate from a detached production owner and general-runtime launch.
Reservations cannot be released from R03-A direct-child exit alone.
Cross-root resource allocation, live owner adoption, detached orchestration,
capture durability/offset ordering, backpressure, public serialization/authority,
watcher adoption, scale and R04 fault families remain separate. B1's receipt records
its original bounded stop; B2 has a separate assignment and receipt.

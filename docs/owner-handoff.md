# Private fixture owner handoff (R03-B2)

Subsequent [R04-C](migration-receipts/R04-C.md) uses fresh schema5 roots, explicit
lease injection and a safe allowlisted spawn/retained PID waiter. Earlier schema
and close-before-spawn descriptions below record the owning stage; current
concurrent isolation uses dedicated record-lock sentinels, not fixture admission
guards. Schema4 and older roots refuse without conversion.

This is a fixed diagnostic composition, not a supported engine runtime, detached
owner service, backend or run/resume/cancel interface. The default scaffold still
reports unsupported. It uses the existing binary's exact private parent, owner and
effect fixture modes on macOS arm64. No second executable or canonical capability
is introduced. Current public fixture selection follows [task reads](task-reads.md);
this private protocol adds no canonical capability or contract delta.

## Scope and launch precondition

B2a supplies private values/codec, validation, deadline-aware channels, descriptor
checks and a qualified fixed ledger. B2b composes B1's durable claim with an actual
owned fixture child, observed identity and acknowledgment. B2c remains separate:
durable raw capture, sync/offset/event/terminal ordering, backpressure and recovery.

Each fixture mode starts as a fresh source-controlled single-thread process. There
are no worker threads, external waiters, ambient Store handles or user-state lookup.
Entry and every spawn reject an environment entry, a surviving descriptor beyond
0/1/2, or a non-pipe stdio endpoint. Descriptor enumeration collects names and closes
the enumeration handle before inspecting surviving descriptors. This is a refusal
policy under the single-thread precondition, not a general concurrent launch barrier.

The parent explicitly initializes a nonexistent fixture store only when requested
for setup. Owner/effect modes never initialize, upgrade or repair roots. Parent
preparation closes every Store/SQLite/lock handle before owner spawn. The owner
claims using a short per-command open/drop adapter, closes the Store before effect
spawn, then reopens for observation/acceptance. No storage lock spans a stream,
process or fixture-gate wait. Each spawned process replaces stdio with fresh private
pipes and clears its environment. Only a fixed mode of the same executable runs.

B1 demonstrated that CLOEXEC does not prevent the spawn-time lock inheritance
window. Closing all storage resources in these fresh single-thread modes, checking
the remaining descriptor set and exercising an independent child Store probe/write
qualifies this narrow ordering. Arbitrary multithreaded runtime launch remains
unqualified. No global unbounded mutex, sleep or retry hides that limitation.

## Private channel and binding

Frames have a four-byte big-endian body length, magic HFB2, version1 and a strict
tag. Maximum total size is24576 bytes, checked before body allocation. IDs/digests
are canonical lowercase hex; process identity is PID/start seconds/microseconds/
boot UUID. Paths are absolute canonical lexical UTF8 without NUL, at most4096bytes;
payload is at most16384bytes. Booleans, stage/fault tags, field lengths, complete
frame consumption and EOF are strict. These are private fixture values, not a
public serialization or idempotency contract.

The owner's Hello contains its observed actual identity. The parent compares it
with the original registered identity of its retained direct child. Invitation
binds operation, store generation, payload digest, owner ID, claim nonce, physical
owner identity and revision. Ready echoes it exactly. Prepared repeats that context,
the complete immutable B1 request and explicit root/qualified fixture configuration.
The owner requires complete Prepared plus EOF, freshly qualifies itself and compares
the full request against the stored snapshot before CAS. Nonce is a concurrency
binding, not a secret credential or hostile-peer authentication.

Partial/malformed/oversized/trailing input or an inconsistent binding cannot claim.
A complete validated Prepared plus EOF may be delivered even if the parent died:
EOF is not evidence of non-execution. Every non-Prepared B1 claim returns NoGrant;
neither replay, observing a fence nor missing acknowledgment grants another effect.
Existing parent preparation returns an observation without launching another owner.

The fixed child receives its operation, explicit fixture probe root, full expected
owner fence and qualified fixed ledger identity. The fence includes its nonsecret
claim nonce; it is concurrency context, never a responder credential. Original
prepared payload, owner protocol pipes and credentials are not forwarded. No credential system or secret material
is introduced; fixture inputs must be nonsecret test data. No decoded request or
payload is logged. Diagnostic failures use fixed categories.

Private protocol errors exit2 silently. Entry may have rejected stdio itself, so
the refusal path neither writes to nor changes flags on an unqualified endpoint.
Exit2 never establishes non-execution; the parent reports uncertainty and observes
durable state after any possible handoff.

## Ordering, deadlines and cleanup

| Boundary | Required ordering |
| --- | --- |
| Parent | Prepare commit; close Store; spawn owner; bind actual Hello; invitation/Ready; Prepared; input EOF. |
| Owner | Validate stored request and actual identity; claim commit; close Store; spawn fixed child; qualify original child identity. |
| Observation | Persist exact child identity using current owner fence; close Store before sending child Start. |
| Fixed effect | Complete Start plus EOF; expected generation/full fence/self identity validation, independent Store probe/open/fixed fixture write; close Store; one qualified ledger append+fsync; EffectDone. |
| Acceptance | Match EffectDone to original child; acceptance commit; close Store; emit Accepted. |
| Parent result | Match echo, original owner and exact durable request/fence/child/AcceptanceRecorded snapshot. |

The parent/owner/effect have one absolute local protocol deadline of10/8/4seconds,
respectively. Frames cannot renew it. Parent observation expiry is not cancellation
of an independent owner. Once handoff or a grant may have occurred, failure/late or
lost acknowledgment stays uncertain and preserves exclusion. No action is queued
for replay after refusal. Spawn, filesystem/SQLite and kernel identity calls are
synchronous nonpreemptible calls; admission checks are not hard wall-clock bounds
on arbitrary kernel/filesystem stalls. Gate connect/read/write use nonblocking
sockets/poll with the same deadline.

After spawning, every persistence/start/acknowledgment failure converges on cleanup
with the retained sole waiter. It closes input cooperatively, then uses finite
observation and separately bounded, freshly qualified direct-child TERM/KILL when
permitted. Unknown original registration is never rebound to gain signal authority.
Unknown wait/control/cleanup cannot release a reservation. Cleanup grace is separate
from protocol admission; it cannot authorize another effect or acceptance.

The fixed effect child has no descendants and its own finite input deadline. It
stays inert until complete Start+EOF, appends at most one record and exits. Owner
death therefore closes its input or leaves a finite cooperative fixture; a test
parent does not acquire wait/signal authority over the grandchild. This safety
property is specific to the fixed fixture, not arbitrary agents or escaped trees.

Exit, capture EOF, overflow and cleanup remain distinct R03-A observations. Protocol
stdout is read directly without capture ingestion; stderr remains separate. No
terminal result, durable drain/history or complete containment is published. No
B1 release call is made by this composition; even a finished fixture keeps its
reservation until separately qualified release semantics exist.

## Fixture controls and ledger

Constructor-injected stages use a separate temporary Unix socket only at the
selected stage. Packet and exact echoed response bind stage/operation/nonce; EOF,
wrong context and timeout refuse. Gate descriptors close before any subsequent
spawn. They are explicit private fixture controls, not environment backdoors or
public lifecycle operations. Stages cover parent pre/post preparation and launch,
owner post claim/spawn/observation, and committed acceptance before acknowledgment.

Deny-only fixture faults model unknown initial child registration and an unusable
child pipe. They never add an execution right, substitute a fresh identity or
release a claim. Injected pipe denial is not evidence of a real OS pipe-creation
failure. Actual successful identity and cleanup paths require independent OS tests.

The destination is the pre-existing test-created exclusive regular file named
r03-b2.effects under the isolated store parent. Open uses nofollow/nonblocking;
regular type, link count1, exact device/inode and a65536byte cap are checked under
an immediate file lock before append+fsync. Records contain operation and actual
child PID/start/boot only. No arbitrary filename or shell command is accepted.
This ledger is an external fixture oracle, not engine capture persistence.

## Dependencies and retained gates

Only the existing exact macOS platform nix0.31.3 wrapper adds socket to its reviewed
fs/poll/process/signal feature set; defaults remain disabled. This supplies safe
nonblocking connect/poll/SO_ERROR without workspace unsafe. Its locked closure adds
memoffset0.9.1 (MIT) and autocfg1.5.1 (MIT/Apache-2.0). No domain serialization,
new runtime framework, native library or global tool installation is added.

See the [B2 receipt](migration-receipts/R03-B2.md) for delivered evidence and limits.
General multithreaded launch, detached owner longevity/adoption, reboot/PID reuse,
owner-death recovery, arbitrary I/O/power faults, full containment, durable capture,
public lifecycle and full R03/R04 acceptance remain outstanding.

## Subsequent B2c delivery

The separate [capture/completion foundation](capture-completion.md) now extends
this handoff with a raw suffix after EffectDone, durable indexed capture and private
terminal ordering. The original B2 limits above are historical; public lifecycle,
reservation release, containment and R04 remain unqualified.

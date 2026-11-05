# R04-B — Production ownership and recovery policy

2026-11-05. Proposed specification; no implementation or owner acceptance.
See [receipt](../migration-receipts/R04-B.md), [R01 architecture](R01-architecture.md)
and [R04-A evidence](../migration-receipts/R04-A.md).

## Inputs and authority

Baseline Engine HEAD47d6505d91ece8ce5f371b3c2dd07b166dee0dfe; schema4 and Rust1.99.1.
Canonical input is HarflowMCP0.0.10 source
8a936de61b5f9f2fdff671b65b5384259267350b, complete207-fixture manifest SHA256
ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906.
This document does not change that pin, fixtures, APIs or capability advertising.

Pinned producer sources:
[contracts](https://github.com/Harflow-Dev/HarflowMCP/blob/8a936de61b5f9f2fdff671b65b5384259267350b/docs/contracts.md),
[supervision](https://github.com/Harflow-Dev/HarflowMCP/blob/8a936de61b5f9f2fdff671b65b5384259267350b/docs/supervision.md),
[outcomes](https://github.com/Harflow-Dev/HarflowMCP/blob/8a936de61b5f9f2fdff671b65b5384259267350b/docs/task-outcomes.md).
Contract anchors are detached ownership (contracts150–184), restart observation
(1375–1396), observer-parent qualification (1451–1471), launch uncertainty
(1980–2005), and dead execution/disk capacity (supervision106–156).

HarflowMCP owns public semantics until accepted O04. New recovery/status/outcome,
permission or retry semantics need producer specification, capability disposition,
fixtures and tests at an immutable commit before exact Rust/consumer adoption.
Internal mechanisms below do not create public authority. No Python-root sharing,
history import, dead-owner pipe adoption, launch replay or app MCP fallback.

## Production launch prerequisite and selected internal direction

Retain R01's dedicated detached task owner and separate scheduling owner. A
mandatory central service changes availability/lifecycle policy and is not selected.
The owner retains the actual agent child, sole waiter, capture and input channels;
observers never acquire these by reading a PID or stored row.

R03-B1 observed lock inheritance under concurrent spawns; R04-A's test admission
guards restore fixture preconditions only. Closing the launching thread's Store,
setting CLOEXEC or serializing one call site cannot isolate other threads' locks
during fork. Production must address every process creation, including helpers,
Git and backend qualification, while root lifetime leases remain held.

Select for qualification a platform-owned process-scoped POSIX record-lock
coordinator plus Darwin posix_spawn with CLOEXEC_DEFAULT and explicit descriptor
file actions. Record locks are not inherited across fork; the spawn allowlist
protects post-exec descriptors. These are separate obligations. Use dedicated
permanent sentinel inodes, never SQLite/WAL files. One process-wide coordinator
owns the only descriptor per sentinel and enforces local shared/exclusive conflicts,
bounded admission and last-reader unlock. Closing any descriptor for the same inode
can release process locks: duplicate opens, independent adapters and hidden closes
are forbidden. Unlink/recreate is not an unlock operation. Process-scoped lock
semantics must be qualified on each supported filesystem/platform.

Do not mix old flock-style and new fcntl-style exclusion. Version the lock protocol
and fail closed on incompatible roots/processes; initial qualification uses fresh
isolated roots. A new marker alone cannot fence old binaries that ignore it. P1
must select an old-binary-recognized incompatible store/schema/format boundary or
prove physical namespace exclusion; test refusal in both directions with actual
old/new executables. No schema change is made here. An offline transition needs
a separate explicit design and proof.
Root shared leases span owner/scheduler lifetimes and retained resources; no
temporary release to permit spawning. Recovery/reset still requires qualified
exclusive quiescence, not a newly created lock path.

Existing nix0.31.3 safe spawn/file-action/wait interfaces are the candidate seam;
return a platform-owned retained waiter around the child PID, without manufacturing
std::Child or unsafe pre_exec callbacks. Validate the Apple flag representation,
file-action ordering, private-channel ownership, executable identity, clean explicit
environment and stdio policy against the pinned dependency and actual Darwin.
Every process-creation path must use the seam or fail admission. Dependencies,
features and targets remain unchanged in this specification.

The pinned safe wrapper lacks chdir spawn actions. Launch the owner by absolute
path; its fresh single-thread bootstrap establishes the validated working directory
before threads, Store handles and agent spawn. Never mutate a multithreaded caller's
CWD. Unexpected ECHILD/external reaping loses wait authority and becomes Unknown.
P1 must settle coordinator placement across storage/platform without a dependency
cycle: application owns the lease port, platform owns OS locks and host injects
the shared coordinator into storage; preserve existing dependency policy or obtain
an explicit reviewed policy revision before implementation.

Primary design grounds: [Apple fcntl](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html)
and [posix_spawn](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/posix_spawn.2.html).
These documents and pinned API inspection establish feasibility, not deployed-OS proof.

Kill criteria: unexplained inherited lock/descriptor, uncontrolled duplicate-open,
mixed-protocol access, lost sole-wait authority or an unmediated spawn path blocks
production launch. Do not respond by weakening locks or adding test guards. A
broker/service alternative requires a new design including child-wait ownership,
failure boundary and owner-approved availability; it is not an automatic fallback.

## Caller, durable handoff and lifetime

Production admission must qualify the actual caller once from trusted transport
and OS evidence, including PID/start/boot and verified ancestry where required.
Caller-supplied IDs, environment markers and diagnostic dispatch_origin do not
establish authority. Unknown/changed identity refuses before effects. A ctl
observer-parent is its confirmed live immediate parent, not the ephemeral ctl
process or a claimed parent. Responder credentials remain outside agent-readable
roots and bind the exact action, root generation and authorized role.

Commit immutable intent, operation/launch generation, reservation and prepared
request digest before external execution. Bind root physical identity, schema,
configuration, backend/session, lineage, saved grants and owner claim. The private
inherited channel carries executable preparation; a stored receipt alone cannot
execute. Owner revalidates binding, prepared digest and current backend/grants,
then atomically claims. Duplicate contenders observe/refuse, never both execute.
Record actual child PID/start/boot before acceptance. Run acceptance is not success.

Lost acceptance, timeout, partial frame or uncertain database completion means an
effect may exist. Preserve generation and reservations, reconcile observations and
require separately authorized retry only after accepted proof of non-execution.
Never replay stored intent, enqueue a successor or fall back because a response is
missing. Native observations must be contiguous through the exact final watermark
before workflow settlement/fallback/join; final parent exit is insufficient.

The owner outlives dispatch/observer processes, with a clean explicit environment,
owned channels and full-lifetime root lease. Observation wait deadlines do not
terminate execution. Execution, input, continuation and background-idle budgets
are distinct, durable across observer restart and not reset by reconnection.
Retain the canonical default600second background-only idle bound after a result
with no foreground turn, pending producer resolution of result classification.
Private fixture10/8/4second lifetimes,64KiB streams and128events are not production
budgets. Before implementation, specify per-operation admission/capture/spool/
event/retention limits, deadlines and exhaustion mappings; measure against
[R01 validation](R01-validation.md), without claiming those budgets are proven.

Drain stdout/stderr concurrently independently of bounded storage queues. Preserve
raw append→declared sync→qualified offset/event commit→durable acknowledgement.
Terminal exit, EOF/drain, capture loss, input reconciliation and containment are
independent facts. Capture failure may coexist with successful drainage and valid
historical terminal evidence; continuation still needs observed exit and complete
drainage. No durable success/exit is fabricated after a failed persistence write.
Production free-space checks occur before reservation and again before owner
execution:256MiB on each distinct state/output, checkout and temp filesystem;
discovery failure refuses. This is not reserved capacity or protection from ENOSPC.

## Authority and recovery transitions

The following names describe internal evidence, not new public enum values.

| Observation / transition | Permitted action and required evidence | Retained boundary |
| --- | --- | --- |
| Prepared, no execution proven | Explicit retry only under pinned pre-spawn rules and exact generation/settings | Missing record/ack is not proof of non-execution |
| Dispatcher dead; owner, capture and input healthy | Attach observation after fresh pinned identity checks; preserve same owner, task and budgets | No control transfer, new child or state rewrite merely to acknowledge a reader |
| Owner live, observation unavailable | Bounded inspect/wait with truthful unavailable/unknown diagnostic | Preserve reservations; no timeout-based replacement |
| Owner dead, agent live or unknown | Preserve evidence; expose lost/uncertain supervision through accepted mapping | Pipes cannot be recovered; no signal/adoption/relaunch from the saved PID |
| Both durable identities positively dead | Same-generation recovery under exclusion and unchanged binding; held launch lock skipped without waiting/probing | Nonempty start identities required; unknown/missing agent blocks, except pinned proven pre-spawn cases |
| Recovery authorized by preceding evidence | Durable identity-bound intent→interrupted task record→settled receipt with pending reconciliation→idempotent follow-up/resource reconciliation→clear pending last | Crash resumes same generation; no continuation/completion notification; unobserved delivery remains unknown |
| Matching terminal record, identities dead, binding mismatch | Pinned human-only binding repair where all prerequisites hold | Preserve outcome/claims; terminal alone gives no release |
| Original PID reused, boot changed or lookup failed | Distinguish positive original absence from Unknown; use complete identity evidence | Neither grants control over the unrelated current PID |
| Exit observed, descendant still holds pipe or escapes group | Bounded drain/control attempt only against qualified owned targets | No fabricated EOF, containment, cleanup or replacement permission |
| Manual resource/session release | Separate explicit human judgment under canonical operation | Not exit/drain proof, task takeover or safe continuation proof |

Protect reservations, session/resource claims, prepared input, captures and recovery
intent while ownership, delivery, containment or reconciliation is unresolved.
Retention expiry/terminal timestamps never erase exclusion evidence. Bound scans
and payloads; return per-entry refusal without hiding unaffected discovery. Damaged
partial associations block actions requiring complete associations. A storage or
inspection failure does not turn retained into free. Root reset is a separate
explicit journaled backup/reset operation; task recovery never implies reset.

## Outcomes, containment and producer decisions

Preserve original observed outcomes; optional Git/artifact/permission observations
do not change execution status. Null/unknown remains distinct from empty success;
Git ranges are local observations, artifact registration is not artifact validity
or task success. Do not re-probe today's repository as an original terminal fact.

The reviewed issues remain requirements/decisions, not desired bug parity:

| Producer issue | Recommended policy and remaining decision |
| --- | --- |
| [#110](https://github.com/Harflow-Dev/HarflowMCP/issues/110): background work after a successful result | Producer should separate result outcome from background/cleanup obligation. Unknown dependency remains required; failed results remain failed. Required monitors must not silently become completed. Exact representation and workflow routing need producer decision/tests before exposure. Do not simply reproduce incidental-leftover failure or invent a Rust success enum. |
| [#112](https://github.com/Harflow-Dev/HarflowMCP/issues/112): dispatcher restart continuity | Apply pinned observation-only attachment when owner/input/capture qualify. Require bounded truthful wait/inspect and actionable lost/unknown supervision. This does not close the issue or recover dead-owner pipes. New next-action/recovery semantics must originate in MCP. |
| [#113](https://github.com/Harflow-Dev/HarflowMCP/issues/113): extra writable paths | Preserve publish boundaries and exact grants; unsupported enforcement refuses with an actionable explanation, never implicit unrestricted access. Producer must settle any tool-level versus OS-enforced permission promise using measured headless backend evidence before relevant backend qualification. External output paths must not be silently moved into the checkout. |

POSIX groups/sessions are an initial control mechanism, not arbitrary descendant
containment. Cancellation needs fresh qualified owned targets, retained waiter and
bounded escalation/drain. Release/replacement additionally needs complete accepted
settlement/containment evidence; process death, successful result or terminal record
alone is insufficient. Default unresolved policy: retain and require attention.
Any narrower cancellation/cleanup promise is a material owner choice, specified
truthfully in canonical contracts before implementation or capability exposure.

Future accepted steering subsets affect R11/R14: preserve request identity,
commit/revision/role fences, issued attempts and explicit safe-boundary authority.
Observation or ordinary context is not steering authority. No ST06 starts here;
do not invent steering wire states or treat pending subsets as accepted contracts.

## Stages and independent qualification

Each stage requires separate implementation authorization. These are proposed
dependency gates, not dispatched migration sessions or acceptance claims.

| Stage | Smallest deliverable | Entry / exit gate |
| --- | --- | --- |
| P1: concurrent process/lock seam | Internal reusable lock coordinator, versioned root exclusion and safe spawn/wait adapter, tested with fixed unpaid helper executables | Reviewed lock protocol, numeric work/deadline limits and complete spawn/open inventory first. Exit only after independent real Darwin concurrency/FD/sole-wait tests; no public runtime/backend. |
| P2: durable owner runtime | Compose existing intent/capture foundations with P1 into detached reusable owner; exact qualified caller/private grant handshake and durable lifetime/ack rules | P1 qualified; explicit production queue/spool/lifetime/retention bounds and failure mapping. Exit after observer/owner crash and every durable transition cut, actual drain/capture loss and final-watermark tests. Fixed helpers qualify mechanics, not a stock backend. |
| P3: recovery and settlement | Bounded same-generation idempotent recovery, protected evidence and separate human judgment paths | P2 plus accepted producer mappings/policies and complete identity/containment prerequisites for affected actions. Observation-only subset may ship internally sooner; control/release stays disabled until separately qualified. |
| Later backend/public integration | Measured backend/grant adapter and exact public consumer behavior | Separate R05/relevant R07 assignments, producer decisions/immutable fixtures, supported platform containment/cancel qualification and eventual R14 acceptance. No present public capability claim. |

Independent verification must use external OS/file/DB witnesses rather than only
the implementation's return values. Preserve exact inputs, barriers, deadlines,
fault points, cleanup inventory and unknown dispositions in later receipts.

| Proof family | Required matrix / independent witness |
| --- | --- |
| Lock/spawn | Concurrent readers/writers plus all helper/owner spawn kinds; independent continuous exclusion probes across spawn while the original holder is live, plus deterministic post-exec helper gates and FD inventory. Post-exec gates do not witness the pre-exec window; any direct pre-exec stop requires separately reviewed OS tracing/instrumentation, never unsafe callbacks or custom fork. Enumerate sentinel identities; duplicate-open/close, local shared/exclusive conflict, mixed protocol and process crash. Zero unexplained exclusion loss or leaked authority descriptors. No arbitrary sleeps as ordering. |
| Claim/acceptance | Two actual parents contend; barriers at intent, claim, OS spawn, identity persistence and acknowledgement. Count actual child effects externally; lost frames/EOF/late commits never produce duplicate execution. |
| Lifetime/caller | Actual parent identity/start/boot and ancestry, spoofed caller, parent exit, dispatch exit, owner exit, observer restart; agent remains uniquely owned while owner lives. Separate model PID reuse/boot/permission faults from real process observations. |
| Capture/terminal | Both streams saturated, bounded spool exhaustion, descendant pipe holders, partial writes/sync errors, input interruption, final watermark gaps and crash around each offset/event/terminal transaction. Read raw bytes and read-only DB independently. |
| Recovery | Owner dead/live/unknown × agent dead/live/unknown; missing durable identity, held lock, damaged binding and recovery crash at every pending step. Verify unchanged generation, retained claims, no signals/replay/notifications and idempotent reconciliation. |
| Outcome/control | Successful/failed result × required/incidental/unknown background obligation; escaped session/group descendants, reused PID and incomplete drainage. Compare accepted producer status/wait/event/routing fixtures; no unsupported control or release. |
| Scale/bounds | R01 workload/budgets plus measured owner RSS, queue/capture/retention caps, cancellation and recovery deadlines. Refuse unsupported storage; document synchronous nonpreemptible syscall limits rather than promise hard wall-clock guarantees. |
| Actual storage faults | Separately authorized disposable size-limited filesystem/VM only; populate that volume to real ENOSPC without consuming host free space. Mount state/output/temp cases independently; externally observe write/close/sync results and last durable record. No setup or exhaustion in R04-B. |
| Failed sync/power | Injected errors establish code-path behavior only. Actual failed-sync/device behavior requires an isolated fault-capable VM/device and named filesystem/cache model; actual abrupt power-cut qualification requires dedicated disposable environment and explicit approval. Restart, inspect durable bytes/DB independently and verify uncertainty/retention. Host kill tests do not prove power durability. |

## Material choices for owner adjudication

1. Recommend retain-and-attend when containment/ownership/capture is unresolved;
   no automatic execution-owner adoption. Cost: capacity may remain occupied until
   qualified reconciliation/manual judgment. Broader adoption requires a separate
   cooperative protocol and lifecycle design; it cannot recreate lost pipes.
2. Recommend producer separation of result and background obligation for #110,
   with unknown treated as required. Cost: coordinated producer/routing work before
   public parity; benefit: no silent weakening of required background work.
3. Recommend preserving strict grants for #113 and refusing unsupported backends.
   If tool-level enforcement is acceptable, the owner must accept its explicit
   measured threat boundary; unrestricted access is not an automatic substitute.

Routine lock/spawn representation and transaction mechanics are selected above
for proof, not delegated as policy questions. Next proposed authorization is P1
only; no R05, public runtime or recovery control is implied.

# Task runtime transaction core

This unqualified implementation stage adds storage/application/interfaces seams for
one unrestricted Claude task. It does not expose public run/cancel, qualify a
production inherited invitation, or establish release compatibility.

`prepare_launch_intent` is the actual private T0 transaction. Its SHA-256-checked
metadata is at most 16 KiB and raw UTF-8 prompt at most 4 MiB. It creates ownership,
lifecycle, immutable intent and reserved launch-attempt rows only. It rotates
paging evidence, never the task watch revision. `prepare_lifecycle` remains a
closed legacy foundation and must not be used by public launch composition.

`publish_launch_spawn` is T2. Under exact owner/root/attempt fence and lifecycle
CAS, it requires the observed original child and a running TaskCreate that
materializes the immutable prompt and metadata. Public task/header/prompt/process
rows, ownership SpawnObserved, launch state and task watch become visible in one
commit. The direct ChildRecorded legacy delta refuses production private intents.

`record_launch_acceptance` separately persists the endpoint/source binding and
AcceptanceRecorded after trusted read-only admission of the child, listener and
raw sources. A path string alone grants no authority. Endpoint syntax is bounded
to the macOS Unix path budget and rejects control characters and dot components.

`publish_task_terminal` admits only original-child observed exit, actual drain and
input closure with a trusted retained-owner join/FIFO proof and trusted raw
capture admission. Sticky cancellation wins; completed requires zero exit and a
successful matched result witness. It atomically publishes final facts, served
header/status, task_finished and EOF log facts, lifecycle evidence and watch
revision. It does not release ownership, infer descendant containment or prove
backend/workflow settlement. Ordinary lifecycle event publication still rejects
arbitrary task_finished.

The interfaces production projection includes canonical unrestricted Claude
 enforcement disclosure, the existing event envelope and the baseline status field
inventory. Unobserved metrics, current responder/owner identity
and raw/event path observations are explicit null/unknown rather than inferred
values. Structured auxiliary surfaces preserve canonical refusal shapes: queued
followups and artifacts are unavailable; approval summary carries the canonical
uncertainty/error with zero placeholder counters; recovery activity reports its
unmet confirmed-capture-loss prerequisite. Those counters do not establish empty
observed inventories. Unavailable followups have a null queued_followup_count.
The native NeedsYou reader deliberately treats the uncertain approval summary as
unreadable; supplying a structured object does not fabricate a clear approval
ledger. Actual ledger observation and runtime owner/capture population still
require qualification before public routing. Task header/status blobs
have a dedicated 16 KiB production cap; row and cumulative read budgets remain
unchanged and SQL sizes are checked before allocation. Historical fixture replacement
keeps its 2048-byte cap. A 2049-byte blob seeded outside that wrapper is now within
the production read bound; such former oversized fixtures need explicit test-owner
reclassification. T0 preflights worst-case spawn and terminal projection sizes.
No missing observation is inferred from a result, timer, stored endpoint or
successful storage call.

`initialize_runtime` initializes only a fresh Rust-owned root using the same
neutral initialization implementation as fixture wrappers, with injected
WriterOptions durability, then records authoritative empty catalog coverage.
Existing roots are not upgraded. `RootHold::revalidate` checks the retained
physical identity without acquiring another lease. `retained_runtime_processes`
provides bounded deny-only owner/child evidence even before public TaskCreate;
complete authenticated caller/peer OS qualification remains a platform gate.

Counted production blob chunks use HFTC-v1, at most 16 KiB payload and 24 KiB
frame. Duplicate/out-of-order/changed total/digest/length transfers refuse. A
completed transfer remains untrusted until storage recomputes SHA-256. HFTI-v1 is
only a closed immutable intent digest helper and is refused by canonical admission.
HFTI-v2 binds exact root physical source/generation, logical ownership and CLI
sessions, task/operation/attempt/reservation, caller/dispatcher identities, ctl/backend
pins, cwd, environment/prompt/metadata/configuration/invocation digests, measured
version and fixed policy/budget versions. Inherited channel, live parent/peer and
physical source checks remain mandatory platform/host admission; decoded bytes
alone grant nothing. HFOP fixture permissions and assignment remain unchanged.

HFTCANCEL-v1 contains only exact lifecycle/request bindings and Cancel; it has no
caller identity assertion. The bounded canonical cancel formatter preserves
result.task_id/status/cascade, always reports incomplete descendant coverage, and
never advertises the cascade capability. Authenticated control transport and actual
signal/wait custody remain platform/host responsibilities.

The strict participant decoder validates every retained preparation. A Prepared row
contributes only its dispatcher identity; the qualified ordinary caller parent is
never agent evidence. After T1, only the retained actual owner/child identities
contribute: that owner must be recorded before every effectful probe, so probe
ancestry remains visible without denying unrelated commands from the same shell.
The exact-current-Prepared exclusion removes only a fully equal persisted request
row; matching identities in other operations remain evidence. Unreadable, legacy, malformed or over-budget preparations
refuse the complete inventory rather than implying no agent. A positively admitted
pre-agent refusal writes only not_started/reason and private lifecycle revision;
claim and T2 then permanently refuse replay. An unsettled probe or unknown commit
cannot supply that admission.

Writer bootstrap uses distinct HFTW-v1 bounded to 16380 payload bytes (leaving
four bytes for the 16 KiB length frame). It binds a canonical absolute root locator,
complete HFTI-v2 invitation, original OwnerClaim and independently observed writer
identity. HFTR-v1 WriterReady binds those identities and lifecycle/ownership
revision 1, and may be emitted only after positively committed OwnerClaimed.
Parsing either message grants no root, process or custody authority; platform/host
must independently compare actual inherited parent/self, binary pins and root
physical source. Truncated, trailing, malformed or over-budget messages refuse.

The host library now composes validation-only owner/writer entry, the private
transaction adapter and the bounded original writer-child handoff. Mutation or
transport uncertainty permanently poisons protocol work while retaining the
original root, process and descriptors. A separate discard tick attempts stdout
and stderr independently without storage or further protocol writes. Cached
WriterReady is historical T1 evidence, not current liveness or cleanup proof.
Inherited stdio is cloned; dropping a channel does not close original descriptors
0/1/2 or prove EOF. No private/public ctl route, complete owner execution loop or
OS-backed admission qualification is established by these source adapters.

Malformed backend output never poisons the writer. The stream validates a line completely
before its result, summary or terminal facts reach the observation. The first malformed line
(undecodable bytes, a boundary or record violation, an oversized line, or a partial last line
at exit) stops decoding, records an observation gap and closes the agent's input; later bytes
are still appended raw and committed. A result line accepted earlier, in the same batch or an
earlier one, stands and storage decides the status; without one the task ends `failed` with
its real exit code. The summary is never published after malformed output. Local failures
(timestamps, counters) still poison.

A full reader queue is a capacity loss, not a discard request. The owner closes the agent's
input once, keeps exchanging the queued batches, and settles with `qualify_with_loss`: every
ordinary settlement constraint holds except that bytes are accounted per stream as read =
delivered + discarded, with the committed ends equal to the delivered bytes, and a requested
discard never qualifies. `Finish` then uses opcode 4 (`capture_loss`). The writer durably
records the known loss before the terminal; if that fails it is poisoned and no terminal
follows. The loss is private evidence (`capture_known_loss`, `loss.raw`, the gap); a task with
a successful result and exit 0 remains publicly `completed`.

Caller-policy order of the dispatcher: admission, preflight, then an existing root is opened
and inventoried, while a missing root gets one more caller observation equal to the admitted
one before it is created (it has no recorded participants). A refusal at those steps creates
no root and writes no T0, probe or spawn. A change seen only at the final observation before
T0 refuses without T0, probe or spawn, and may leave an initialized empty root.

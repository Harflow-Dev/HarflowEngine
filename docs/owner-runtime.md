# Owner-runtime candidate

HarflowMCP remains the canonical contract owner. Its local commit
`4f5b86d27d249094d7378c84029a94e2aaec966d` specifies the candidate
`owner-runtime-v1` component, with manifest SHA-256
`7df0c93a17b185620e35fcf0d0a3e060174ff0445318ed505dc7e4573a08c6d6`.
Rust vendors that component separately from the supported engine contract inputs.
The 64 specimens are synthetic model inputs. They do not advertise Python runtime
support, public Rust execution support, authenticated caller evidence or real
permission enforcement. The supported MCP 0.0.10/full207 baseline remains distinct.

## Approved policies

Owner decision D5 of 2026-11-06 confirms that these approvals stand. Where the historical
[R04-B](migration-receipts/R04-B.md) and [R04-D3](migration-receipts/R04-D3.md) receipts
describe these policies as unaccepted or pending, this section supersedes them.

Unresolved ownership, capture, containment or resource reconciliation retains the
original actors and claims for attention. It does not authorize automatic
adoption, replay, replacement, cancellation or release.

Agent result, required/incidental background obligations, workflow disposition and
resource settlement are separate. Unknown obligations are treated as required;
current-attempt failure is preserved. A successful result does not prove exit,
closed pipes, reconciled input, lossless capture or released claims. Evidence
completion is an observation, not a release grant.

The production filesystem boundary requires measured OS enforcement for the exact
binary, configuration, grants and protected paths. Unsupported or unmeasured modes
refuse. Tool-level enforcement is separately named and requires explicit opt-in;
it does not silently weaken strict mode. Network, publication, base freedom and
exact command grants remain independent admission constraints. Hypothetical JSON
qualification observations cannot qualify a real backend.

Storage runs in one child process per operation so an admitted storage call cannot
block the owner main loop. This introduces no central service and makes no
universal bounded-kill or bounded-shutdown promise under arbitrary OS I/O.

## Private implementation boundaries

The domain/application policy model is pure. The interfaces layer strictly checks
JSON structure, numeric ranges, duplicate keys, Unicode and explicit budgets before
model decisions. Unknown optional fields are tolerated within those budgets;
unknown effect-bearing enums refuse. Process identity comparisons use only the
known PID, start time and boot fields.

A closed `HFOP1` preparation encodes immutable policy, profile/configuration digests,
operation/session/reservation/attempt/generation and caller/issued-attempt context
inside the existing digest-covered prepared request. It is for fixed unpaid
mechanics qualification. It does not manufacture a production caller grant or
certify OS enforcement. Learned writer identity is not rewritten into preparation.
Accepted steering checkpoint/revision/issued-attempt context remains distinct from
control authority; neither old output nor storage acknowledgments prove withdrawal,
retirement or replacement. Python steering bridges Mac migration; its accepted
semantics also belong in Rust. This component does not implement the entire later
backend/steering stage.

The owner holds a root-only token without a SQLite connection. The writer holds
its own independent root token through raw-file and SQLite close. An immutable,
private, single-link participant sidecar binds the learned writer to the original
request, source generation, owner and channel. It is deny-only provenance, with no
recovery or release authority and no schema-5 upgrade.

The private `HFOR1` exchange permits one pending command. It binds sequence,
participant digest and actual encoded command bytes; full acknowledgments must
match projected snapshots, capture state and stable raw-file identities. Ambiguous
send, timeout, malformed/stale reply, refusal or unknown commit poisons future
mutations permanently. A later reply cannot restore admission. Closing input stops
future commands but cannot cancel an already admitted syscall. Close is a separate
healthy command; it must not be interleaved into partially sent command bytes.

The owner incrementally transports at most 4096 bytes per direction and writer
stderr per step, with one poll of at most 20 ms. Full frames are capped at 24576
bytes and a single-stream append at 16384 bytes. Writer stderr retains 4096 bytes
and drains at most 1 MiB before becoming unavailable; it is not agent output or
death proof. Startup root qualification, spawn and identity probes can enter
nonpreemptible OS calls; the poll limit is not a universal wall-clock bound on
every setup step. The independent reader has four bounded batches and a sticky discard
state. The consumer retains a batch until both streams are acknowledged. EOF is
published only by an empty append after reader join, FIFO reconciliation and input
reconciliation; final capture and direct-child wait remain separate.

The conservative logical raw-payload envelope is 240 KiB: the earlier 192 KiB
reader/consumer envelope, 16 KiB TX and two 16 KiB writer payload representations.
This is not an RSS, stack, metadata or kernel-pipe bound. Existing schema 5, 64 KiB
per-stream raw cap, 128-event cap, reader limits, eight-second owner/writer,
four-second fixed effect and ten-second dispatcher are qualification limits, not
invented production defaults. The candidate background-only idle default remains
600 seconds; it is not a new foreground hard timeout.

Normal teardown requires both original children actually reaped, reader joined,
accepted FIFO reconciled and writer channels physically complete. Semantic
acknowledgment and physical EOF are different evidence. An unresolved actor keeps
the owner/root resident; durable claims remain unresolved even if actors later
become physically quiescent. Original child handles retain sole wait/signal
ownership. No PID-only fallback, stored-row adoption or process-tree containment
claim is introduced.

## Remaining production evidence

Policy approval and fixed-helper implementation do not qualify stock agent
backends. Real production caller lineage and exact grants, protected-path
OS enforcement, backend/version/configuration measurements, descendant containment,
production numeric envelopes, platform support and actual storage-fault behavior
remain separate evidence gates. Private owner/process mechanics are macOS arm64
only; participant creation explicitly refuses non-Unix platforms. Unsupported paths must refuse with an actionable
reason. No public execution capability is advertised by this candidate.

Canonical changes precede Rust adaptation. Consumer vendoring, integration,
acceptance, publishing and releases remain separate decisions. The active release
freeze forbids pushes and releases.

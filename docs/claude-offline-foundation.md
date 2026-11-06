# Offline Claude backend foundation

Status: implemented and synthetically verified; no production admission.
HarflowMCP remains the canonical engine and public contract owner. This work is an
independent preparatory R05 slice, not acceptance of R03, R04 or R05.

## Evidence and scope

The preceding [scripted protocol measurement](claude-scripted-protocol.md) records
the exact Claude 2.1.318 local text exchange. Its startup failures, compatibility
profile and successful measurement remain historical evidence. This foundation
uses bounded synthetic records and injected observations only. It does not rerun
the vendor, authenticate a provider, execute tools, or extend security grants.

The selected supported baseline remains MCP 0.0.10/full207 at
`8a936de61b5f9f2fdff671b65b5384259267350b`. Owner-runtime-v1 remains selected at
`4f5b86d27d249094d7378c84029a94e2aaec966d`, manifest
`7df0c93a17b185620e35fcf0d0a3e060174ff0445318ed505dc7e4573a08c6d6`.
Reading canonical behavior references does not select a newer contract.

## Observation boundaries

Invocation planning is one-shot and offline. The pinned no-tools argument
constructor supplies the shape; a custom prompt and its declared configuration
digest are synthetic planning inputs, not a rerun or extension of the measured
nonce protocol. Live, resume and production modes explicitly refuse. A declaration binds the prepared
request, original caller/grant context, root identity, profile, runtime version,
logical and physical executable paths, executable digest and framed invocation
configuration. Matching these values establishes fixture correlation; it does
not authenticate a caller or establish production runtime closure. The recorded
source and physical facts are supplied independently as synthetic observations;
this stage does not prove a transcript came from a captured native process. The original
caller can differ from the detached owner.

The fixture decoder caps each pushed chunk at 4,096 bytes, each line at 24,576
bytes, the stream at 65,536 bytes and normalized records at 128. These are
qualification-fixture bounds, not production resource envelopes. A final line
without its newline refuses as truncated.

JSON transport decoding belongs to interfaces. Backend reduction consumes pure
normalized records. Passive tool observations are correlated data and never
callbacks or tool authority. CLI session identity is distinct from the Engine
session. Root and child scopes cannot complete each other's obligations.

A backend terminal result, exhaustion of recorded input and physical settlement
are independent observations. A malformed suffix invalidates the trace while
preserving an already observed explicit failure. Missing or uncertain evidence
stays unknown. Success cannot synthesize input reconciliation, capture EOF,
complete inventory, containment or settled writers.

The read-only owner bridge consumes accepted snapshots and capture progress.
Only capture supplies process exit, pipe EOF and loss. Pending or poisoned
storage, missing/mismatched owner evidence and cancellation intent prevent a
successful projection. Capture progress revision is separate from the original
owner fence revision. Independently injected settlement facts must match their
context. Existing owner policy projects observations; it grants no release,
replacement, replay or launch authority.

HFOP1 still permits only its fixed/saturated fixture assignments. Those assignments do not
represent a Claude launch. Production requests refuse before the
injected observation port is called, even with favorable synthetic data.

## Remaining qualification and decisions

| Gate | Required next evidence or decision |
| --- | --- |
| Real backend owner launch | Explicit accepted preparation/assignment/launch contract binding executable, configuration, caller/grants, owner identity and trusted captured-byte provenance; current fixed-effect assignment is insufficient. |
| Caller and roots | Authenticated caller/grant composition, exclusive production-root ownership and exact runtime/configuration closure. |
| OS enforcement | Mode/version/platform-specific enforcement and denied-boundary evidence, descendant containment and resource envelopes. |
| Tools and continuation | Separately authorized tool/live-input/resume qualification, bounded consumption and durable continuation semantics; offline events do not qualify these. |
| Storage and recovery | Fault/unknown-outcome, cancellation, containment and settlement qualification with independently trusted witnesses. |
| Acceptance and delivery | Explicit aggregate migration acceptance and production admission; client selection, acquisition, registration and publication remain separate. |

No new permission is needed to finish this offline slice. The next real launch or
provider/tool qualification requires an explicit bounded assignment and its
prerequisites. No public capability or `QualificationState::Passed` is created.

## Verification receipt

The final locked Rust workspace passed 398 tests with 17 ignored. All 20 new
offline tests passed within that suite, including real-SHA synthetic host
composition. Formatting, locked check, all-target Clippy with warnings denied,
architecture/legacy-name/whitespace guards and 22 Python tests passed. The default
scaffold still reports engine operations unsupported. Independent plan/final
review converged within three rounds; the final test fixtures use reachable
settled capture revision 2 below owner fence revision 3. No vendor/provider test
was enabled, and no hosted CI or additional platform qualification is claimed.
See the [stage receipt](migration-receipts/R05-offline-foundation.md).

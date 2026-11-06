# Prepared fixed-helper launch foundation

Status: implemented and verified with fixed local helpers. This is a private mechanical
foundation, not backend qualification or aggregate R03/R04/R05 acceptance.
HarflowMCP remains the canonical contract owner; supported and candidate pins are
unchanged. Actual host composition remains macOS arm64; other platforms do not
expose these private process modes. Pure values, codecs and injected application
checks do not establish native platform qualification.

## Canonical semantics and internal representation

The canonical 0.0.10 reference at
`8a936de61b5f9f2fdff671b65b5384259267350b` already separates immutable invocation,
private authorization transfer, durable reservation, original caller and detached
owner. Durable files cannot execute a task. Acknowledged launch acceptance does
not mean completion, delivered output, settlement or retry permission. Unknown
send/transport/acknowledgment never becomes proof that execution did not occur.
The source references and the precise remaining decision are in
[the bounded Claude experiment](decisions/isolated-claude-owner-qualification.md).
No producer source or public contract is changed here.

`HFPL1` embeds the unchanged `HFOP1` policy bytes in a distinct closed private
preparation. It admits only the source-owned fixed protocol helper. Its frozen
record includes executable logical/physical paths, on-disk file identity/length/
change metadata and SHA-256, the exact single helper argument, empty environment,
physical CWD, complete root source stamp, qualified effect ledger, source profile,
and original caller/grant/attempt context. Configuration hashing uses explicit
framing and excludes the configuration digest itself; the outer request hash
covers the entire wrapper. Unknown assignment/version, changed fields and malformed
or over-budget payloads refuse. Neither a backend name nor arbitrary argv is a
launch option.

HFPL1 is built only after fresh schema5 initialization. `initialize=true` with an
HFPL wrapper refuses before initialization or preparation mutation. No schema5
conversion or stored-row adoption is added. Legacy HFOP1 bytes and fixed-helper
selection remain separate.

## Actual owner composition

The private parent validates the prepared request, current physical root/ledger,
original caller and executable observation before preparation. Only the distinct
HFPL route selects the isolated owner. The owner verifies its private invitation,
actual immediate-parent caller, full held root stamp and current executable before
writer/bootstrap/claim, then again before the closed helper spawn. Original caller
and detached owner remain different identities.

The existing single-owner claim CAS, retained direct-child handle, `RecordChild`,
raw-file creation, reader transfer, private Start input and acceptance ordering
remain in force. The helper accepts only the private Start plus EOF, exact stored
spawn fence and its own registered identity. After the same fixed DML/ledger probe
used by existing qualification helpers, it emits identity-bound `EffectDone` and a
deterministic text-only JSONL tail. It executes no prompt, provider, tool or model.
The framing prefix is stripped before raw persistence. This framing is not a
contract a real backend can satisfy.

Executable checks are bounded on-disk observations. The closed helper route
currently refuses logical symlink paths. Path/hash bookends and a helper self-check
do not atomically identify the mapped executable image. A path swap or earlier
replacement cannot establish arbitrary-backend execution identity. Real backend
and production dispatch remain refused.

## Original byte commitment and current source validation

Later file hashing alone cannot prove original capture: a same-inode/same-length
rewrite made before a read would pass ordinary read bookends. The original owner
therefore hashes each raw tail byte exactly once before its immutable append is
submitted. No extra raw payload is retained for hashing. The hashes become usable
only after every append is unambiguously acknowledged, input/FIFO/reader are
reconciled, both original children exit zero, writer channels close, and final
capture is healthy and settled. Poison, loss or uncertain work suppresses the
receipt.

The new private `CaptureAccepted` frame binds the original invitation/nonce/owner,
full prepared request and source stamp, accepted ownership/capture/fence/child/raw
identities, accepted byte counts and both stream digests. Its one-use delivery
adapter reads only from the original retained child's private channel, matching
the expected peer/invitation and physical EOF. Missing or lost delivery has no
stored-file fallback. A record cannot reconstruct this volatile receipt.

The application exact-source reader requires that live receipt. It compares
qualified recovery snapshots before and after the raw read against the original
receipt, reads only the fixed raw filenames at exact accepted extents, and checks
both byte hashes against the owner's commitment. The platform reader requires
regular single-link files, expected device/inode, nofollow/nonblocking open and
unchanged descriptor/path metadata including length/mtime/ctime before/open/after.
Truncation, extra tail, replacement and changed bookends refuse. A same-length
pre-read rewrite refuses through the original byte commitment.

Current recovery event count/digest are validated and must remain unchanged over
the read interval. The receipt commits original raw bytes and final capture; it
does not claim to commit the original entire event-history digest. Returned bytes
are a bounded historical observation, not execution, replay or release authority.
The parent composes this source validation with the original delivery before
forwarding the private receipt. This is not a public launch reply or production
admission.

## Bounds and outcomes

Existing limits remain: 16-KiB preparation, 24-KiB frame, 16-KiB append, 64-KiB raw
per stream, 128 capture events, four reader batches, 4,096-byte transport steps,
20-ms poll and existing descriptor/root coordination limits. Fixed dispatcher,
owner/writer and effect deadlines remain 10/8/4 seconds. Read-only binary hashing
uses at most 128 MiB of input with 16-KiB chunks and a streaming hash; this is a
qualification input limit, not a memory allocation or production runtime closure.
Receipt delivery keeps the original handoff deadline; the two-second exact-source
observation window starts only after that live delivery, capped by the same outer
deadline. Nonpreemptible
OS I/O can exceed an observation budget; no universal bounded-kill/shutdown claim
is introduced.

Conclusive local validation refusal before this invocation attempts a spawn is
not proof about an earlier attempt. Once delivery/spawn/commit is uncertain,
claims remain retained and no automatic replay, cancellation, replacement, release
or root conversion is permitted. Existing rows never grant a new private handoff.
Unsettled actors keep the original wait/control/root boundaries. The HFPL1 parent
retains and drains its original owner until positively reaped; it does not apply
the legacy fixture TERM/KILL cleanup to an owner retaining unresolved work.

The next material decision is the single bounded owner-hosted Claude experiment
specified in the linked brief. No vendor/provider/auth/tool run, new sandbox grant,
production admission, install, registration, client selection, push or release is
included in this foundation.

## Verification

The final locked Rust workspace passed 410 tests with zero failures and 17 ignored.
All 12 new focused tests passed: three codec, four injected application and five
native fixed-helper checks. Native coverage includes actual claim CAS, private
owner/capture, pre-claim/initialization refusal, raw-file mutations and an owner
held past the legacy signaling window before its original zero exit. The deadline
regression uses recorded deadline arguments without sleeps.

Formatting, locked check, all-target Clippy with warnings denied, architecture/
legacy-name/whitespace guards and 22 Python checks passed. The default scaffold
still reports engine operations unsupported. Independent review converged in
three rounds, with final post-fix workspace verification. Native tests required
macOS process-identity access; the initial restricted run's unknown child identity
was not treated as a pass or repaired by weakening caller checks. No vendor/provider
or new OS-exception qualification test was enabled. No hosted CI or additional
platform qualification is claimed. See the [stage receipt](migration-receipts/R05-prepared-launch-foundation.md).

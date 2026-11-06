# Claude isolated transport: one replacement-wave candidate

Status: owner approved implementation and one fake-only measurement on 2026-11-06.
Implemented at `d652778`; the authorized vendor measurement ran once and failed.
See the [result and retained evidence](../claude-replacement-wave-negative.md).
No production admission or further runtime run is authorized.
Claude support remains required. R06 independent work is paused at a plan-only
checkpoint while this transport blocker is investigated.

## Problem and evidence

The categorized fake-only measurement at `807c443` admitted two tunnels. Each
forwarded 1,482 client bytes and 1,132 reverse bytes, then recorded complete
bidirectional EOF, flushed buffers and half-closes. The helper recorded handshake
`transport_eof` for both, not a certificate alert. A third connection triggered
`ExcessConnection`; the CLI did not finish within its original 15-second deadline.

Source review distinguishes an experimental cap from a product security policy.
Only normal relay reads set EOF; only completed buffer drain permits write
half-close. Cleanup sets socket-closed flags, not these transfer flags, and poison
prevents further relay steps. Thus the recorded EOFs preceded global poison.
This excludes third-socket poison as the cause of those already-recorded EOFs;
it does not identify their cause or the third socket's destination or purpose.

The current negative oracle requires an explicit helper certificate alert plus a
completed CLI certificate refusal. Some clients may reject at another layer and
close without an alert. EOF alone cannot establish rejection and the predicate
must not be relaxed merely to obtain a pass. Local differential controls check
helper certificate/handshake, ALPN fallback and CONNECT transparency. A secure
local verifier can also leave a transport I/O failure on rejection: the proxy
intentionally poisons on that error. Generic `Io` alone does not identify reset,
broken pipe or a shutdown error; native closed-kind metadata is needed. A universal clean-proxy-EOF requirement is therefore
a fixture assumption requiring qualification, not a TLS protocol guarantee.
That observation does not identify the pinned Claude EOF reason.

## Exact proposed changed boundary

After the offline controls, the owner authorized one separately reviewed fake-only
measurement with **two concurrent, four total** post-preflight IPv4 sockets.
Four total provides capacity for one hypothetical complete replacement wave for
the two observed completed sessions. Their relationship is not established. It is not a general startup allowance or authority for subsequent waves.
No higher concurrent count or longer observation is proposed.

- One nonce-bound preflight, then at most four total sockets, at most two active.
  An active slot is reusable only after both directions have EOF, drained buffers
  and completed half-closes. Original socket handles must close and drop before
  reuse; identities and counters move to retained immutable metadata and never
  recycle. An unsettled slot cannot be reused merely because accept ran first.
- Each CONNECT must pass the unchanged exact authority/header grammar. The only
  upstream is the original owned numeric IPv4 loopback helper. No DNS, provider,
  real upstream, credential broker or real credentials.
- A fifth total socket, third simultaneously active socket, IPv6, wrong route,
  malformed header, I/O error or aggregate exhaustion globally poisons forwarding.
  Rejected sockets remain unread. Concurrent-cap exhaustion is a candidate limit,
  not evidence of TLS incompatibility. No silent retry or cap escalation by controller.
- Two fixed active slots, 8 KiB header each (16 KiB active header storage), 8 KiB
  payload partition per active slot per direction (16 KiB aggregate pending per
  direction), one 16 KiB scratch buffer, four retained metadata records. The
  256 KiB aggregate raw-read allowance per direction spans all four admissions.
- A distinct local helper admits at most four total/two active TLS sessions with
  the existing fixed 25-second readiness lifetime. Ordinals and peer tuples bind
  exactly to all proxy slots; no missing, duplicated or foreign sessions accepted.
  DONE counts cover actual admitted sessions (one through four), not the budget
  of four; never-admitted slots receive no fabricated timeout/session verdict.
  It emits only closed category/phase, identities, flags and bounded counters.
- Original 130-second overall, five-second preflight, 15-second version and
  15-second negative observation deadlines stay unchanged. No deadline reset per
  wave. Exact Claude/ICU pins and interpreter/TLS settings stay unchanged.
- Original no-tools/no-fork/filesystem/network profile, private roots, literal fake
  token, canaries, denied listeners, complete original-child exit/physical EOF,
  helper/proxy joins, bounded preflight/listener/helper-pipe FD custody, socket
  closure and bounded post-settlement fake-token scan
  remain mandatory. Failed roots remain retained.
- No host trust changes, added CA to Claude, verification disable, raw exception,
  request/response/header/payload capture, TLS-key/debug logging or content parsing.

## Decision and disposition

The discriminating question is whether one replacement wave produces the same
closed handshake outcomes or permits a completed CLI refusal within the same
absolute observation. Counts and chronology cannot identify an HTTP purpose.
A repeated EOF remains a non-certificate outcome. Exhaustion of this candidate is
not permission for another larger experiment.

The existing strict negative predicate remains unchanged; the measurement may
fail while still distinguishing transport behavior. This candidate is a diagnostic,
not an expected fix or a promise of qualification. No live authentication,
provider request, production qualification or R05 acceptance follows even if it
passes. If explicit-alert behavior proves an invalid fixture assumption for the
pinned client, a replacement verifier oracle requires its own evidence and review;
this proposal does not preapprove one.

Before the single authorized vendor run: finish the new offline controls, review
their actual results and freeze the implementation. The approval covers only this
changed-bound fake-only measurement, not any later run or further gate.

## Remaining Claude gates after this diagnostic

1. Establish a sound pinned-client transport/certificate negative oracle and a
   completed original CLI observation. The current EOF result does not pass it.
2. Separately qualify authentication and allowed egress: explicit current authority,
   account-bound volatile broker, exact upstream binding, credential non-persistence
   and failure handling. This stage permits no credential or upstream access.
3. Qualify the actual runtime's caller/grant/configuration closure, no-tools/no-fork
   enforcement, structured protocol/capture/input settlement, resource bounds and
   containment under the accepted owner contract. A local handshake is not this proof.
4. Complete affected owner recovery/unknown-outcome/retained-claim and launch/release
   gates with independent effects and exact runtime/platform evidence. No automatic
   replay, adoption, replacement or release follows from an incomplete observation.
5. Only then review production admission and aggregate R05 acceptance separately.
   No capability, selected canonical pin, client selection, install or release changes
   are part of this proposal. Existing Mac/Python behavior is unaffected.

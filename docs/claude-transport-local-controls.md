# Claude transport local differential controls

Status: offline differential controls implemented and verified. No new vendor run, production admission
or selected runtime change. The prior [category measurement](claude-tls-error-classification.md)
remains a failed strict negative control. Claude support is required; this stage
investigates the transport blocker rather than accepting it as optional.

## Scope and interpretation

The tested transport retains the two-tunnel proxy behavior and fixed
Python 3.9.6 / LibreSSL 2.8.3 helper. Each isolated instance still admits only two
post-preflight connections, with unchanged aggregate byte/pending budgets and
25-second helper lifetime. Local controllers use explicit cleared environments,
retained original child custody, bounded capture and complete exit/EOF settlement.
No vendor, real upstream, provider, credential, model or tool is invoked.

Per-context trust of the generated certificate is confined to the local positive
control. Default verification remains enabled in negative controls. No host trust
store, Claude trust, profile grant, TLS verifier or production mode changes.
The trusted local peer checks hostname, certificate SAN/self-issued identity,
TLS 1.2 negotiation, nonempty cipher and ALPN fallback with an offered h2/http1.1
list. The helper does not advertise ALPN; no-selection must not itself prevent
this local handshake. None of this makes the self-signed certificate trusted by
Claude or identifies the prior client's failure.

## Source-supported chronology

The new deterministic proxy regression completes both opaque exchanges, waits
for clean settlement, and then opens the rejected third socket. EOF, drained-buffer
and half-close evidence survives global poison. Inspection confirms cleanup sets
socket-closed flags only; ordinary transfer and its pre-poison checks establish
the EOF evidence. Thus the two prior admitted sessions had already reached EOF
before the third connection triggered poison. This establishes chronology, not
why Claude closed, a request purpose or universal product connection policy.

## Evidence and review

| Local control | Observed result |
| --- | --- |
| Direct trusted peer | TLS 1.2, cipher present, offered h2/http1.1 with ALPN none; hostname/SAN/self-issued certificate checks, application data |
| Direct default verifier | Client verification exception; helper handshake certificate-alert rejection |
| Two trusted peers through CONNECT | Both application-data outcomes; exact peer tuples; both directions EOF, drained and half-closed; clean joined proxy |
| Curl default verification through CONNECT | Original exit 60 with complete pipe EOF; helper `handshake / certificate_alert_unknown_ca`, no application data |
| Curl closure at proxy | Intentional global `Io` failure; native `NotConnected` and `ConnectionReset` observed in separate local runs; incomplete transfer flags retained |
| Raw EOF companion to curl | Separate exact peer binding, handshake `transport_eof`, no certificate rejection |
| EOF-before-third-socket regression | Two completed opaque exchanges retain genuine EOF/half-close evidence after the unread third socket triggers global poison |

Curl's explicit client verification failure plus the helper certificate alert proves
that at least this local rejecting client does not satisfy the fixture's universal
clean-proxy-EOF assumption. It does not prove how Claude verifies or why it closes.
The native closure interleaving varies: one observed run retained reverse EOF only
and `NotConnected`; another observed `ConnectionReset` before either EOF flag.
The test requires exact verification refusal, helper category/tuple, failed proxy,
native kind presence, closed/joined custody and buffer/EOF structural invariants.
It deliberately does not pretend that one native closure interleaving is universal.
The existing strict vendor predicate is unchanged and remains unqualified.

The supported implementation fix is diagnostic only: payload read/write/shutdown
failures previously discarded native `io::ErrorKind`. The two-file correction
preserves that closed enum in the existing outcome field. WouldBlock, all failure
predicates, forwarding, budgets and cleanup are unchanged. Zero-length write and
non-I/O policy failures still have no invented native error. No raw error strings
are emitted; the local driver catches unexpected failures with a fixed marker.

Two independent source diagnoses informed the reviewed plan and this bounded
metadata correction. Final affected verification passed: two optimized local host
controls; nine two-tunnel proxy and six existing egress controls; formatting, scoped
Clippy, workspace check, architecture/legacy guards and 22 Python tests. No new
installed-Claude run occurred. Full all-target Clippy remains outside this scoped
result because of the preserved unrelated peer-owned ST04 lint.

Evidence: `/private/tmp/harflow-claude-transport/final-host.log`,
`final-proxy.log`, `native-kind-observation.log` and the explicitly transcribed
`closure-variation-observation.md`. The latter preserves the observed variant from
a tool result after the first verification log was replaced by the final run.
The pinned Claude historical banner reports Bun 1.4.4 (424a542a9). Public GitHub
lookup did not resolve that revision; current upstream source is not proof of the
embedded runtime's behavior. No speculative runtime compatibility fix is made.

All 17 protected peer/tracker files remain byte-identical. Independent final
review closed without remaining findings. No R06 code was started, and no public
contract, capability or selected pin changed.

Final host log SHA-256: `41ef7a79b74b53eb700126df66dba64ef62002a03da370f346b239e556feea5d`.
Final proxy log SHA-256: `3e076a6682f58e728581ba91b1eb0aca0d0783c1bf24d062767660669a157d6c`.

Kim's implementation preference was preserved at the routing gate: configured
GLM `glm-5-3` was rechecked, but the unchanged broad absolute-command Bash
allowlist prevented bounded write preflight. No Vibe smoke/task/configuration
change followed. Native Sol low handled the two-file correction; the dedicated
Luna medium writer owned tests, with primary integration and verification.

## Next decision

The complete [replacement-wave candidate](decisions/claude-transport-replacement-wave.md)
keeps two concurrent connections and proposes four total for one hypothetical
replacement wave, with original deadlines and shared budgets. It remains
unauthorized and unimplemented. The strict negative predicate is unchanged;
transport EOF remains non-certificate evidence. No repeated unchanged vendor run
or larger allowance follows automatically. Remaining Claude gates are consolidated
in that candidate, separately from aggregate R05 and production acceptance.

R06 was paused at a reviewed draft plan before any implementation writer started.
Its independent queue design and unresolved refinements remain outside the product
repository. ST04 and migration tracker files remain peer-owned and preserved.

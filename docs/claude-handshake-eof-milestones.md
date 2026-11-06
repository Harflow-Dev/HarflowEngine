# Repeated Claude handshake EOF: native milestone diagnosis

Status: implemented and independently reviewed. One same-bound discriminating
fake-only installed-Claude measurement completed; the strict gate remains failed. The prior [four-total result](claude-replacement-wave-negative.md)
remains failed; no retrospective certificate classification is possible.

## Question and boundaries

Two independent source diagnoses found client closure during the initial TLS
exchange best supported, while certificate verification versus exact-runtime
compatibility remained unresolved. The existing `Handshake` phase only says that
`do_handshake()` did not return; it does not identify the last native protocol event.
CONNECT corruption or fifth-socket poison causing those already-retired EOFs is
poorly supported by the source and retained settlement witnesses.

This discriminator keeps four total/two active connections, the original overall,
preflight/version/negative/helper deadlines, shared byte/pending/header/scratch
budgets, exact vendor/ICU pins and isolated no-tools/no-fork profile. It adds no
trust, cipher, ALPN, TLS-version, verification or upstream change. No credentials,
provider, production admission, push or release is permitted.

## Closed observation

The pinned CLT Python 3.9.6 / LibreSSL 2.8.3 exposes `_msg_callback` and
`sni_callback`. The new observer registers each actual accepted socket with its
ordinal and numeric peer before progress, up to four records. Native callbacks
set a constant-size 12-bit allowlisted milestone mask and a fixed SNI category.
Callback payload and version arguments are never inspected, copied, hashed,
parsed, stringified, stored or logged. The library callback covers TLS protocol
messages, not application data. SNI is compared only against the existing fixed
authority; no raw name is retained or emitted and no context or alert is selected.

Known incidental record-header and ticket callbacks are explicitly classified.
Unknown native descriptors, unregistered identities, overflow or callback errors
produce closed failure metadata. Callbacks return None and handle runtime errors
so observation does not deliberately change handshake outcome. Unsupported
instrumentation cannot qualify. A zero-event admission still retains its identity.

`TLS_MILESTONE_V1` rows follow the unchanged READY/SESSION_V2/DONE transcript and
carry only ordinal, literal loopback address, peer port, mask and SNI enum. A strict
suffix codec validates complete one-to-one ordinal **and** port coverage, known
bits/enums, ordering and the original 4096-byte capture bound. The old codecs and
modes retain their prior behavior. The distinct ignored diagnostic entry requires
`HARFLOW_RUN_HANDSHAKE_EOF_DIAGNOSTIC=1`; it selects only the existing fake-only
four-total proxy and the instrumented local helper. The strict certificate gate
is unchanged, with valid additional metadata also required.

A write flag means the native library reported that protocol event; it does not
prove wire delivery, client receipt or acceptance. An unset bit means **not
observed**, not proof of wire absence. SNI match is not certificate validation.
Proxy forwarding/EOF, helper exception classification, original process settlement
and native milestones remain separate witnesses. None turns EOF into rejection.

## Local controls and result

The optimized local target passed three tests in 0.43 seconds. Actual closed rows
passed the Rust suffix codec against all four independently recorded peer ports:

| Local peer | Native milestones observed | Independent outcome |
| --- | --- | --- |
| Trusted completion | ClientHello, full initial server flight, client key exchange, both CCS/Finished; SNI matched | application data, no certificate rejection |
| Default certificate verifier | ClientHello, full initial server flight, inbound alert; no client key exchange or Finished observed; SNI matched | explicit certificate-alert rejection |
| Generated ClientHello then write EOF | ClientHello, full initial server flight; no client key exchange, Finished or alert observed; SNI matched | handshake transport EOF; certificate verification never performed by this peer |
| Raw EOF | no milestone events observed, SNI unseen; admission identity retained | handshake transport EOF, no certificate rejection |

The initial server flight here comprises native write reports for ServerHello,
Certificate, ServerKeyExchange and ServerHelloDone. The deliberate early-close
peer drains bounded bytes without consuming them through its client TLS object;
it does not verify or reject the certificate. This prevents the matching EOF
pattern from becoming invented certificate proof.

The controls additionally cover a registered valid callback with poisoned payload
and version arguments, unknown/unregistered descriptors and overflow, zero-event
registration, strict suffix identity/enum/mask validation, and no sensitive error
echo. Cleared environments, fixed CLT interpreter, bounded original process
capture/exit/physical EOF and nested helper wait/reader custody are retained.
Unexpected test-driver failures emit fixed markers.

Final checks passed 13 scoped tests: three new milestone controls and ten
original-mode codec/helper tests, including the unchanged 25-second partial
coverage case. All three installed-vendor entries remained ignored during local
verification. Formatting, scoped Clippy, workspace check, architecture/legacy
checks and 22 Python tests passed. Independent final source review closed clean.
All 17 protected peer/tracker files remain byte-identical. Native Sol low handled
implementation and a dedicated Luna medium writer supplied controls; primary
integration strengthened the closed metadata oracles. Kim's unchanged broad Bash
allowlist prevented write preflight; no Vibe task or configuration change followed.

## Single discriminating installed-Claude result

At implementation `fb5fbfa`, the reviewed ignored entry ran once and failed in
18.32 seconds. The four-total/two-active mode and all original clocks/budgets and
pins remained in force. This was a new native-milestone discriminator, not an
unchanged repeat or expanded allowance.

Every one of the four tuple-bound sessions reported the same observations:

| Witness | All four admitted sessions |
| --- | --- |
| ClientHello received by native library | observed |
| SNI fixed comparison | matched |
| ServerHello, Certificate, ServerKeyExchange, ServerHelloDone native write reports | observed |
| ClientKeyExchange, client CCS/Finished, server CCS/Finished | not observed |
| Inbound / outbound TLS alert | not observed / not observed |
| Original helper classification | handshake / transport_eof, no certificate rejection, no application data |
| Proxy transfer | 1,482 bytes forward and 1,132 reverse per session; both EOF/drained/half-closed |

These native observations match the deliberate early-close control. They differ
from the local explicit certificate-rejection control, where an inbound alert was
observed. The failure is localized to **client abort during the initial server-auth
flight, before any client key exchange was observed**. It is consistent with trust
rejection or exact-runtime TLS policy/compatibility failure; it does not distinguish
those causes or prove certificate verification occurred. Application-driven
cancellation also remains possible. Outgoing native reports
plus byte forwarding still do not prove which message the client consumed.

The four proxy upstream-local/helper-peer bindings were `127.0.0.1:51004`,
`:51005`, `:51008`, `:51010` for ordinals one through four; the frozen helper
was `127.0.0.1:50994`. The proxy recorded one preflight, five accepted IPv4 sockets
and four CONNECTs, peak active two and retired/closed four. The fifth socket
caused `ExcessConnection` at 3,907,647 microseconds from worker start and stayed
unread. No admitted EOF was manufactured by that poison or cleanup.

The vendor produced no captured bytes and did not finish within the original
15-second negative deadline. Original exit was unobserved and pipe EOF incomplete
before identity-bound Signal(9) cleanup. Static certificate-refusal evidence and
observation-within-deadline were false. The helper independently exited Code(0)
with complete stdout/stderr EOF and valid complete milestone metadata. The proxy
joined with all owned sockets closed. Denied listeners remained quiet, IPv6
attempts were zero, canaries were unchanged and the bounded post-settlement
fake-token scan completed with zero matches. Peak pending bytes were 1,482/1,132;
aggregate raw bytes were 5,928/4,528, within the unchanged shared limits.

## Disposition and remaining evidence

The observation narrows where the handshake stops; it is not a supported
transport behavior fix or a certificate-refusal qualification. Server-only
milestones cannot establish the client's verification/error decision. The next
needed evidence is a correlated **closed client-side TLS failure reason** under
the same restrictions. No such witness is available from this run's empty CLI
capture, so no trust-versus-compatibility claim is made. More identical retries or
larger caps are not selected. No oracle, deadline, profile, trust store, pin or
production mode was changed.

The historical embedded banner identifies Bun 1.4.4 (424a542a9), whose exact source
equivalence remains unverified. The [public Bun 1.4.4 release](https://github.com/oven-sh/bun/releases/tag/bun-v1.4.4)
shows a different revision; its version label alone cannot attribute an upstream
TLS bug to this embedded binary. No runtime substitution or upgrade was attempted.

All settlement/controls and the bounded scan preceded the strict assertion.
No real credentials, broker, real upstream/provider/model/tool calls, paid calls,
production admission, install, registration, push or release occurred. No further
run followed; failed roots remain retained.

## Retained evidence

- Log: `/private/tmp/harflow-eof-milestones/run.log`.
- Log SHA-256: `e01e2cc7d5a8d63e66a73f36ae4ab60c833b288b6c141d48d451a3d19fb4258d`.
- Root: `/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-81a919108523ce24c723842a35917470`.
- Root device/inode/mode: `16777230 / 605337255 / 0700`.
- Proxy port: `50995`; nonce is the root suffix above.
- Profile SHA-256: `c897e576304ac7ba8089471fd9cbebb574c056e54c0dcf88ef5effb0131e96dc`.
- Vendor SHA-256: `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
- ICU SHA-256: `666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549`.

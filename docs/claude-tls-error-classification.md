# Offline TLS failure metadata

Status: offline classification implemented and verified. No new installed-vendor
measurement is authorized by this stage. The failed [two-tunnel result](claude-two-tunnel-negative.md)
remains a generic TLS-error observation; its missing details cannot be recovered
from the retained log or reclassified retrospectively.

## Closed helper metadata

The private helper emits `SESSION_V2` with ten fields: tag, ordinal, literal
loopback IP, peer port, certificate-rejected flag, application-data flag, outcome,
closed flag, phase and category. READY and DONE records retain their bounds. The
collector rejects old SESSION records, unknown values, incomplete identities and
inconsistent status/category/phase/boolean pairs. This is private test metadata,
not a public contract, capability or product policy.

Phase is `handshake` or `application_read`, determined from saved state after the
actual handshake call succeeds. No error message text or byte count infers phase.
Categories come from exception subclasses and exact allowlisted SSL library/reason
pairs; missing or unfamiliar SSL fields yield `tls_unclassified`. WantRead and
WantWrite remain internal control flow and do not end a session.

Only the existing explicit peer certificate-alert reasons can set
`certificate_rejected`. Local certificate-verification exceptions, other peer
alerts, protocol failures, generic I/O, EOF and timeout remain non-rejection.
`observed_eof` names a zero-length read; default ragged-EOF suppression means that
it does not prove TLS close-notify. An externally stopped helper cannot invent a
controller-stop session result: original nonzero exit and incomplete output stay
failed independently of any records already received.

The collector and helper store/emit only closed enums, booleans, numeric identities
and bounded control metadata. No raw exception, args, library/reason text, request,
response, decrypted HTTP, token, TLS record or key log is emitted. The closed reason
map is source-controlled in the helper; the Rust enum and strict consistency checks
are source-controlled in `codec_categories.rs` and `codec.rs`.

## Runtime and verification limits

The existing fixed helper interpreter was reobserved as Python 3.9.6 linked to
LibreSSL 2.8.3, supporting TLS 1.2 but not TLS 1.3. The separate
`/usr/bin/openssl` certificate-generation CLI reports LibreSSL 3.3.6; it is not the
Python socket TLS backend. This describes the local helper;
it does not attribute the previous vendor failure to a protocol version. No
interpreter, SSL option, trust store, certificate generation, connection cap,
aggregate budget, deadline, custody, global-poison or success predicate changed.
The vendor and provider paths are not run by these offline controls.

The implementation routing preference for Kim was rechecked. GLM `glm-5-3` and
Vibe 3.1.0 were available, but the existing broad absolute-command allowlist and
absent publishing deny entries prevented the required write preflight. No smoke,
Vibe task or configuration mutation followed. A scoped native Sol low implementer
and dedicated Luna medium test writer performed the allowed fallback work.

The fixed local controls distinguish these outcomes:

| Local fixture | Phase | Category |
| --- | --- | --- |
| Trusted application data | application_read | application_data |
| Client rejects the helper certificate | handshake | certificate_alert_unknown_ca |
| Raw TCP EOF | handshake | transport_eof |
| Idle until the original helper deadline | handshake | deadline_timeout |
| Malformed TLS record | handshake | protocol_unrecognized |
| Two fatal-alert byte fixtures before ClientHello | handshake | protocol_unrecognized |

The last two rows reflect the exact structured `SSL` / `UNKNOWN_PROTOCOL` pair.
The pre-ClientHello byte fixtures do not demonstrate interpretation of a peer
alert. The real local client verification exception independently classifies as
`local_certificate_verification_failed`, with certificate-rejected false. Other
reason-map rows are synthetic structured-exception table controls, not observed
TLS outcomes. Missing or unlisted values remain unclassified.

Final post-recovery verification passed: six optimized codec tests, the actual
local TLS helper control (26.10 seconds), scoped Clippy with warnings denied,
formatting, architecture and legacy checks, workspace check and 22 Python tests.
The installed-Claude entry remained ignored. Independent final source review found
no remaining blockers. The crash-interrupted test-label handoff was repaired before
this final run. All 17 protected peer/tracker files remained byte-identical.

Workspace-wide all-target Clippy remains outside this scoped result: the existing
peer-owned ST04 helper has an unrelated `unbuffered_bytes` finding. No global
Clippy success or production qualification is claimed.

## Proposed next measurement — not authorized

After the offline controls and final review pass, one future measurement would be
discriminating: run the **same** pinned two-tunnel fake-only negative entry once,
collecting only the new per-session category and saved phase. The question is
whether the previously generic failures are an explicit peer alert, a local
protocol failure, EOF or still unclassified. It is not an attempt to establish
HTTP purpose, identify the exact startup function or permit a third connection.

The required separate owner approval would cover only this metadata-schema delta:

- Existing pinned Claude/ICU/Rust, fixed helper interpreter, fresh private roots,
  fake token, unchanged no-tools/no-fork/read/write/network isolation.
- One preflight and at most two admitted IPv4 CONNECT sockets for the whole
  instance, exact existing authority/headers, actual upstream exclusively the
  original owned loopback helper. No DNS, external upstream, broker or credentials.
- Existing aggregate 256 KiB per direction, 16-KiB pending storage per direction,
  one fixed scratch buffer and all original absolute observation deadlines.
- Third socket, IPv6, malformed/other route or exceeded bound still globally
  poisons forwarding. Rejected sockets stay unread. No retry or allowance reset.
- The same independent tuple-bound session witnesses, complete original exit and
  physical EOF, proxy/helper joins and closures, denied listeners, canaries and
  bounded post-settlement fake-token scan. Failed roots stay retained.
- No raw errors, payload capture, debug logs, decrypted-content inspection or
  reinterpretation of generic categories as certificate rejection.

The old strict certificate-alert/CLI-refusal/all-session predicates remain required
for qualification. A third connection, unclassified result or incomplete observation
still fails; even an identified non-certificate alert cannot qualify. No runtime
run, live-auth transition, provider call, cap increase, install, registration,
production admission, push or release is selected by this proposal. Additional
experiments would require new discriminating evidence and separate authorization.

## Authorized category/phase measurement — 2026-11-06

The owner subsequently authorized exactly one same-bound two-tunnel fake-only
measurement at `f2a43a5`. The optimized installed entry ran once and failed in
17.03 seconds. No runtime or bound changes preceded it.

| Witness | Observation |
| --- | --- |
| Helper sessions 1 and 2 | `handshake` / `transport_eof`; TLS error, certificate false, application data false, closed |
| Session peer bindings | `127.0.0.1:64506` and `127.0.0.1:64507`, exactly matching proxy upstream-local tuples |
| Frozen helper address | `127.0.0.1:64498` |
| Proxy preflight / accepted / CONNECT | 1 / 3 / 2 |
| First global failure | `ExcessConnection` at 1,956,253 microseconds since proxy worker start |
| Each slot client-to-helper read / forwarded | 1,482 / 1,482 bytes |
| Each slot reverse read / forwarded | 1,132 / 1,132 bytes |
| Aggregate peak pending forward / reverse | 1,482 / 1,132 bytes |
| Proxy custody | original worker joined, all owned sockets closed, both directions EOF/half-closed/flushed |
| Helper custody | original Code(0), complete stdout/stderr EOF, no overflow/errors |
| Vendor observation | original exit unobserved, deadline expired, zero captured bytes, no pipe EOF before cleanup |
| Vendor cleanup | identity-bound Signal(9); incomplete observation remains failed |
| Static certificate refusal / observation within negative deadline | false / false |
| Denied listener / IPv6 attempts | quiet / zero |
| Canaries / bounded fake-token scan | unchanged / complete, zero matches |

Preflight isolation, exact vendor version/hash, ICU hash and metadata bookends
passed. This is evidence of handshake transport EOF at the helper, not an explicit
peer certificate alert. It does not establish why the client closed, its request
purpose, or whether closure preceded or followed global poison. The third socket
remained unread; its destination, content and caller are unknown. No retrospective
reclassification of the earlier run follows. Strict qualification remains failed.

The original vendor, helper and proxy custody settled before the final scan and
assertion. All 17 protected peer/tracker files remain byte-identical. No real
credentials, broker, real upstream/provider call, paid call, content logging,
production admission, push or release occurred. No retry, cap increase or further
runtime experiment is selected by this receipt.

Retained evidence:

- Log: `/private/tmp/harflow-two-tunnel-categories-run/run.log`.
- Log SHA-256: `77134169551bbdf3f4d97a2229f1ad5ed55dbf8e1031140d323a789ebaeb569f`.
- Root: `/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-3c3e696b6423438baa95bb988465d234`.
- Root device/inode/mode: `16777230 / 604449042 / 0700`.
- Proxy port: `64499`; profile SHA-256: `fea9ef04d3550c472af54fc80d817fda3ed7d8474f6727829696c7f4a63305d4`.
- Claude 2.1.318 SHA-256: `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
- ICU SHA-256: `666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549`.

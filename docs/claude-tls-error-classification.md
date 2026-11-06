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

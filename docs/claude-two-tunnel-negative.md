# Two-tunnel fake-only Claude TLS control

Status: implemented and independently reviewed; one owner-authorized fake-only
measurement failed on 2026-11-06. Strict TLS qualification remains unestablished. The [reviewed design](decisions/claude-two-tunnel-negative-proposal.md)
is the scope contract. No production backend, public operation, live authentication
or aggregate migration acceptance follows from this private control.

## Implementation boundary

`TwoTunnelProxy` accepts only a frozen numeric IPv4 loopback helper address. Its
constructor has no live mode, DNS or provider destination. The new host entry is
`installed_claude_two_tunnel_tls_negative`, gated by
`HARFLOW_RUN_TWO_TUNNEL_TLS_NEGATIVE=1`; its call graph supplies a literal fake token
and cannot call the credential broker or the real-credential phase. The original
one-tunnel and live entry points keep their behavior; only the existing exact route
validator's visibility was widened for reuse. No dependency, unsafe code, public
contract, pin or vendor OS grant changed.

One nonce preflight precedes two total post-preflight socket admissions, without
slot recycling. A third attempt, IPv6, malformed route/headers, wrong authority,
duplicate preflight, I/O fault or exceeded bound poisons the whole instance.
Headers are bounded and processed incrementally, while both validated tunnels
progress. Only the approved CONNECT envelope is parsed; TLS contents stay opaque.
No headers, payloads, invalid destinations, raw errors or secrets are logged.

Raw read allowance is shared at 256 KiB per direction. Payload storage uses two
fixed 8-KiB slot partitions per direction, with a single 16-KiB scratch buffer;
peak pending counters provide numeric evidence without storing traffic contents.
The one-thread worker retains every original socket, checks denial/deadline state
before forwarding and joins before reporting closure. Ready-for-collection is an
advisory hint that resets during acceptance and on failure; only the joined
outcome and per-direction EOF/drain evidence can qualify.

The helper is one original retained process with at most two concurrent sessions
and the existing fixed 25-second readiness lifetime. Each verdict carries its
literal loopback peer IP/port. A bounded eight-field codec checks unique contiguous
helper identities and exact matching against proxy upstream-local tuples. Missing,
duplicate, foreign or incomplete sessions cannot become certificate rejection.
The helper distinguishes explicit certificate alerts from generic TLS/I/O errors,
EOF and timeout; it detects application-data presence without parsing HTTP.

The original 130-second overall, five-second preflight, 15-second version and
15-second negative-phase caps remain. The negative deadline is computed before
vendor spawn and reused while awaiting natural proxy settlement. A sticky
observation flag records whether that completed before the same deadline;
controller cleanup never repairs an expired observation or incomplete CLI capture.
Original identity/exit/physical EOF custody, helper/proxy joins, listener/control
witnesses and a bounded post-settlement literal-fake-token scan precede assertions.
Failed roots and canaries remain retained.

## Verification and implementation routing

Implementation began at `8b6cd9` with native Sol low workers for the proxy and host
lifecycle. Dedicated Luna medium test writers own socket, codec and helper tests.
During implementation the owner requested Kim for subsequent code writes. Existing
writes were preserved. Vibe 3.1.0 and configured GLM `glm-5-3` were available, but
local Bash's broad absolute-path allowlist and absent publish deny entries prevented
clearing the required write preflight. No Vibe task, smoke or configuration change
was performed; authorized native integration fallback handled review corrections.
This implementation-routing check did not run a product provider qualification.

Independent review found and corrected absolute deadline precedence, an eight-field
helper/seven-field collector mismatch, and premature controller stop before final
proxy EOF observation. Final affected verification passed; the single vendor result is recorded below.
Concurrent ST04 and migration tracker files are excluded from this stage.


## Final verification

The locked workspace test run passed **447 tests, zero failed, 27 ignored**. This
includes concurrent ST04 source present during the build and is not a clean-release
or hosted-CI claim. Final test-only improvements were followed by affected reruns:
**eight** optimized two-tunnel tests, **six** original egress tests, **four** codec
tests, **one** actual helper-control test and **two** native host input tests passed
(two existing vendor measurements ignored in that target). The actual helper
control includes its fixed 25-second timeout and controller-stop negative. Both
directions' aggregate limits, active-flow IPv6/third-socket poison, invalid routes,
non-recycled admission and idle partial headers have offline witnesses.

Formatting, architecture/legacy guards, locked workspace check, 22 Python tests,
workspace all-target Clippy excluding hf-platform, and scoped hf-platform library,
original egress and new proxy target Clippy passed. Full hf-platform all-target
Clippy was separately attempted and remains blocked by the preserved unrelated
ST04 `tests/st04_helpers/support/case.rs:209` unbuffered-bytes lint. No assertion or
production lint was weakened to obtain these results. Final independent review
round three approved exactly one fake-only installed-vendor run after the fixes.

## Single installed-vendor result

The exact optimized ignored entry ran once and failed in **17.08 seconds**. Its
preflight/version controls passed: no-tools/no-fork/no-spawn, outside-read and
protected-write denials, allowed/denied listeners, exact vendor version/hash, ICU
hash and metadata bookends. Both admitted tunnels progressed and subsequently
reported complete bidirectional EOF, flushed buffers and half-closes. The third
accepted IPv4 socket caused the first proxy failure, `ExcessConnection`; it was
closed without request parsing. The connection allowance was not raised again.

| Witness | Observation |
| --- | --- |
| Proxy preflight / accepted / CONNECT | 1 / 3 / 2 |
| First failure | `ExcessConnection`, 2,513,813 microseconds since proxy worker start |
| Slot 1 proxy local / helper peer | `127.0.0.1:51891` |
| Slot 2 proxy local / helper peer | `127.0.0.1:51892` |
| Frozen helper peer for both proxy slots | `127.0.0.1:51877` |
| Per-slot client-to-helper read / forwarded | 1,482 / 1,482 bytes |
| Per-slot helper-to-client read / forwarded | 1,132 / 1,132 bytes |
| Aggregate peak pending client-to-helper / reverse | 2,964 / 1,132 bytes |
| Per-slot helper result | `tls_error`, no explicit certificate alert, no application data, closed |
| Vendor observation | Ready, deadline expired, original exit unobserved, zero captured bytes, no pipe EOF before cleanup |
| Vendor cleanup exit | `Signal(9)`; does not repair incomplete capture |
| Static CLI certificate-error witness | false |
| Proxy observation before negative deadline | false |
| Proxy custody | joined; every owned socket closed |
| Helper custody | original Code(0), complete clean stdout/stderr EOF |
| Denied IPv4 listener / IPv6 attempts | quiet / zero |
| Outside-read / owner-write canaries | unchanged |
| Bounded post-settlement literal fake-token scan | complete, zero matches |

This shows that the two-tunnel mode allowed actual relay progress beyond the
previous immediate poison. It establishes neither certificate rejection nor
acceptance: `tls_error` deliberately does not classify its underlying TLS reason,
and no application-data observation identifies a request purpose. The third
socket's destination, contents and caller remain unknown because it stayed unread.
No conclusion about retry intent, successful authentication or billing follows.
The strict control failed before cleanup and remains failed after settlement.

The run used a separate negative-only call graph, with both older experiment gates
unset. No credential inventory/read, broker, real upstream, provider request, paid
call, tool/fork grant, production admission, install, registration, push or release
occurred in this cycle. Original vendor custody settled after identity-checked
cleanup; helper and proxy settlement preceded the scan and assertions. The failed
root and sibling canary were retained. No automatic rerun or cap increase followed.

## Exact retained bindings

| Binding | Observation |
| --- | --- |
| Base | `8b6cd930ae669bfec76da7b7484d03eb38b5a7d9` plus this reviewed implementation and preserved peer source |
| Platform | macOS 26.6.2, build25G83, arm64, reobserved after the run |
| Vendor | `/Users/nayanda/.local/share/claude/versions/2.1.318`, 319598464 bytes |
| Vendor SHA-256 | `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d` |
| ICU SHA-256 | `666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549` |
| Proxy port / nonce | `51878` / `ff2440760c1054d9135bae61fb44c618` |
| Profile SHA-256 | `d7bc6e63f08abd3cb3159d07647c4a6339bf04e68526a46c86d76b53f08eb368` |
| Root | `/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-ff2440760c1054d9135bae61fb44c618` |
| Root device / inode / mode | 16777230 / 604115641 / 0700 |
| Log | `/private/tmp/harflow-two-tunnel-run/run.log` |
| Log SHA-256 | `45bdce880dbdd9dc7864bfda895d813909c9b959def711aafa9b77436b7f7790` |

All 17 inventoried peer/tracker files remained byte-identical. A useful next
question is the fixed TLS-error category; a reviewed metadata-only classification
would be needed before proposing another discriminating measurement. Repeating
the same run or increasing the connection cap would not resolve that missing
witness. No new experiment is selected or authorized by this receipt.

## Offline classification follow-up

[Closed TLS failure metadata](claude-tls-error-classification.md) adds offline-tested
phase and category evidence for a separately authorized future measurement. It
does not reclassify this historical run or authorize another installed-vendor run.

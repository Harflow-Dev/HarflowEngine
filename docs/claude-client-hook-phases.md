# Claude hook-entry and settlement diagnosis

Status: implementation and independent review complete; the single fake-only
measurement ran and remains unqualified. The previous
[client-error observation](claude-client-tls-reason.md) remains unqualified:
READY alone did not establish whether fetch was called, fulfilled, rejected or
unsettled. This stage adds bounded phase metadata to that same diagnostic hook.

## Observation boundary

The first four global fetch invocations retain their original arguments,
receiver, return values and error objects. The function Proxy still forwards
properties including `preconnect`; no property, socket, TLS, Bun.fetch or other
transport hook is added. Promise identity, timing and unhandled-rejection effects
remain an acknowledged diagnostic limitation. No production qualification follows.

Protocol 2 retains nonce/PID-bound READY, original-error category, overflow and
final records, and adds six-byte phase rows within the same fresh retained file.
Short rows inherit the validated header's nonce/PID binding; they are diagnostic
data, not bearer authority or a socket association.

| Record | Positive observation | What it does not establish |
| --- | --- | --- |
| `CI2` | The apply trap was entered before native invocation | Native invocation completed or opened a particular socket |
| `CN2` | Native invocation returned a result before observer attachment | Fulfillment, rejection or a valid promise |
| `CS2` | Native invocation threw synchronously | A particular TLS failure reason |
| `CF2` | The fulfillment handler ran; the same value is forwarded | Response content, request identity or TLS/socket association |
| `CE2` | An original error reached the observer and was mapped to closed code/cause categories | An original CLI certificate refusal or per-socket certificate alert |
| `CA2` / `CU2` | Observer attachment or internal classification failed | A native transport failure reason |
| `CO2` | The first call beyond four was observed and passed through | Coverage of later calls |
| `CX2` | The observer emitted its final stream report | Every call settled, all later process activity was observed, or the final descriptor close succeeded |

The parser enforces contiguous entry IDs, each call's ordering, one origin and
one terminal record, exact final counts and fault/health consistency. Reentrant
calls and asynchronous completion can interleave. A valid incomplete prefix
preserves positive observations; absent final counts remain unknown. A completed
healthy stream can still contain unresolved calls. An entry without a terminal
means no recorded settlement, not proof that its promise remained pending.
Absence after a recorder failure cannot prove absence of calls. Hook replacement
or alternate native paths remain outside coverage.

No arguments, response properties, message, stack, URL, header, body or TLS key is
logged. Error classification continues to inspect only bounded own data-property
codes and one cause code, without getters or coercion. Marker failures cannot
replace the native result or original error.

## Bounds and unchanged gate

The writer remains append-only with existing no-follow/device/inode/mode/link/
size checks and at most one reporter descriptor open at a time. No in-place
writes, new files, timers or broader hooks are introduced. At nonce length 32,
PID length 10 and category length 13, the maximum is exactly:

`CR48 + 4 × (CI6 + CN-or-CS6 + CE78) + CO48 + CX56 = 512 bytes`.

Short fulfillment or fault records replace the larger error record. There are
at most 15 rows. The two 512-byte files and 3072-byte helper capture preserve the
4096-byte metadata allowance; the retained reader's overflow probe is unchanged.

The exact Claude 2.1.318/vendor/ICU pins, no-tools/no-fork OS profile, four total/
two active connections, 256 KiB aggregate per direction, 16 KiB pending per
direction, 16 KiB scratch and 8 KiB header per slot remain unchanged. Deadlines
remain 130 seconds overall, five preflight, 15 version, 15 negative and 25 for the
helper from its original spawn. The instrumented version phase must qualify
before any negative execution.

Original CLI streams alone feed the existing refusal predicate. Every admitted
session still requires explicit certificate-alert evidence. Observer metadata
cannot substitute for either condition. Original child/helper/proxy custody,
pins, source canaries, denied listener controls and the bounded fake-token scan
precede final assertions. Failed roots are retained. Exactly one fake-only
measurement is authorized after offline tests and final independent review.
No credentials, provider calls, production admission, push or release are granted.

## Offline verification

Eight Rust parser regressions passed, including exact 512-byte acceptance,
513-byte refusal, per-call/reentrant ordering, contradictory faults and valid
unresolved prefixes. Both fixed Node contract/writer programs passed through the
retained original-child runner in 0.16 seconds, with cleared environment, fresh
HOME/TMPDIR, five-second deadlines, 512-byte output caps, typed original identity,
Code(0), physical EOFs and empty streams. The writer boundary uses a synthetic
JavaScript PID only within the offline fixture; it changes no OS identity or
vendor pin. Classifier, recorder and attachment fault injection preserves native
outcomes. No offline result substitutes Node for the pinned Bun runtime.

The 25 Python checks, formatting, workspace check, scoped relevant-host Clippy,
architecture/legacy guards and whitespace checks passed. UI is not applicable.
The unrelated peer ST04 all-target Clippy limitation remains outside this scoped
claim. Native Sol low implemented the adapters, a dedicated Luna medium writer
owned regressions, and the primary independently verified the checks.

## Single measurement receipt

Implementation: `ac5e59a`. Independent plan review and fresh final source review
closed clean before execution. On 2026-11-06 the one release-mode
`fake_only_client_reason` measurement failed in 19.04 seconds. The metadata-only
log SHA-256 is
`98d85b3ee5afb2b9ce365a4dbf58c28beab8bc1e1513af5facda7bdad69dc5de`.

The instrumented version phase qualified: original PID 68597, start
1793968452.204219, Code(0), exact 22-byte version, empty stderr, complete physical
EOFs and a healthy 94-byte protocol-2 report with zero recorded version calls.
It was part of this measurement, not a separate repeated support probe.

Negative PID 68625, start 1793968453.252158, wrote a valid nonce/PID-bound 43-byte
`CR2` READY row only. No entry, native-return, synchronous-throw, fulfillment,
error-category, attachment/classification-fault, overflow or final row was
recorded. The strict parser returned no recorded calls or errors,
`reported_calls: None`, and incomplete/unqualified health. This is not a positive
observation of recorder failure or a proven zero-call total. The original CLI
streams were empty; the original 15-second negative observation expired with no
observed exit. Retained cleanup settled the original with Signal(9); its custody
path requires original exit and physical EOFs before returning. Cleanup does not
repair the expired observation into success.

The four admitted sessions again reported Handshake/TransportEof. All had matched
SNI and native server Certificate/ServerHelloDone write milestones, without
observed client key exchange, CCS, Finished, alert or application data. Each
forwarded 1482 bytes toward the helper and 1132 toward the client, with empty
buffers, physical EOF/half-close and socket closure. Peak active slots remained
two; all four retired normally. A fifth accept was rejected as ExcessConnection
at 4,118,789 microseconds from proxy start, without upstream admission. The proxy
joined and closed all owned sockets. The original strict gate failed at the
negative proxy deadline; CLI refusal and per-session certificate-alert conditions
also remain unsatisfied.

The original helper PID 68507 exited Code(0), with complete observation and
physical EOFs, 479 bytes stdout, empty stderr and no capture errors. All source,
vendor/ICU bookends, canaries, denied IPv4/IPv6 listeners and bounded fake-token
scan passed before assertions. Candidate source/test hashes and all 17 protected
peer files remained unchanged. The failed root and its outside canary are retained:
`harflow-os-measurement-709142dc8d423f6fc2931fb2fd91f227`.

The new observation is **no recorded hook entry**. It does not establish whether
the TLS path bypassed the hook, a later global replacement occurred, or recording
became unavailable after READY. Successful short-record emission inside the pinned
Bun process remains unproven; offline Node controls do not fill that coverage
gap. The TLS failure reason remains unknown. No additional run, broader hook,
content logging, credentials, provider calls, production admission, push or release
followed. All original caps, isolation, deadlines, budgets and strict predicates
remain unchanged.

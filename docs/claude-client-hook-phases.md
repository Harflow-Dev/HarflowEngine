# Claude hook-entry and settlement diagnosis

Status: implemented and offline-verified; final independent review pending.
No measurement has run in this stage. The previous
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

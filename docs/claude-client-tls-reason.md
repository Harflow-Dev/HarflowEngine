# Client-side TLS reason: preload observability qualification

Status: inert support probe passed; the single reviewed fake-only client-error
measurement ran and remains unqualified. No client TLS failure reason was captured.
The [native milestone diagnosis](claude-handshake-eof-milestones.md) remains
unchanged: client closure before observed key exchange is not certificate proof.

## Safe capture boundary

Two independent static diagnoses identified a fixed runtime preload as the most
promising client-side observer, but strings in the exact pinned binary do not
prove that loading is supported. [Bun's standalone documentation](https://bun.sh/docs/compiler/executables#runtime-arguments-via-bun_options)
describes runtime options; [compile feature settings](https://bun.sh/reference/bun/CompileBuildOptions/functionSource)
can disable preloading. The embedded revision is not assumed equivalent to a
public release. Claude's own `--preload` command is not evidence of arbitrary Bun
module loading. No different runtime, debugger or compiler/security override is
selected for the pinned probe.

Content-bearing debug-file, NODE_DEBUG, verbose-fetch and TLS-key logging are
excluded. An observer's certificate-code text must never enter CLI bytes scanned
by `certificate_error()`: that would risk manufacturing a refusal witness.
A future observer needs separate bounded metadata, own data-descriptor code
classification and explicit unknowns, without messages/stacks/request arguments,
URLs, headers or bodies. Normal socket error listeners can change handling;
errorMonitor is preferable when supported. Fetch wrappers can change promise
identity and microtask/unhandled-rejection behavior, so they require an explicit
diagnostic limitation and review. None is implemented or qualified by this probe.

## Inert support probe

`installed_claude_client_preload_support` is separately ignored/gated by
`HARFLOW_RUN_CLIENT_PRELOAD_PROBE=1`. It runs only the pinned Claude `--version`
under the unchanged vendor profile. Preflight denials/controls remain, then one
15-second version deadline covers both initial pin observation and version
capture. The original 130-second overall bound remains. There is no TLS helper,
model request or CONNECT; an owned unused target listener provides a quiet witness
and the existing four-total/two-active proxy remains bounded and joined.

A fixed CJS reporter is created in the existing readable, deny-write protected
fixture area and bound by source identity/hash bookends. A parent-precreated
0600 metadata file has one retained no-follow/CLOEXEC descriptor. The reporter
requires supported nonzero O_NOFOLLOW, regular/nlink-one/device/inode/initial-empty
checks, makes one bounded marker write and closes its descriptor. Only the
explicit file/nonce/device/inode environment keys and native PID are read. No
request hook, error message, network operation or CLI output is involved.

The exact READY marker binds a fresh nonce and typed retained ProcessIdentity PID;
no debug-string parsing is used. Parent reads are capped at 512 bytes with identity,
mode, link-count and stability checks. The 512-byte side channel uses part of the
unused existing 4096-byte helper-metadata allowance; version pipe limits and FD
hard limits are unchanged. CLI stdout/stderr remain solely original capture.

Original process exit/physical EOF, proxy join, target/denied listener quietness,
canaries, source/vendor/ICU bookends and bounded fake-token scan precede support
assertions. An expired observation cannot qualify. Missing/malformed metadata is
**not proven usable**, not a particular compile-time diagnosis. It blocks the
client observer and selects no fallback flags, extra probes or TLS run. A valid
marker proves bootstrap execution only, not error-channel coverage or TLS safety.
Failed roots remain retained.

## Local verification

The metadata file and marker regressions validate real filesystem identity,
replacement/symlink/hardlink/size refusal and exact non-echoing nonce/PID parsing.
A preinstalled Node fixture validates only the fixed CJS file-writer contract with
cleared environment, five-second/512-byte capture and typed native process identity.
It is not substituted for Claude or used as evidence of Bun loading support. An
outer-tool-sandbox identity refusal was resolved by the appropriate local test
escalation; no timer, identity workaround or vendor profile rule was added.

Nine scoped checks passed: four filesystem tests, one strict parser test, one
explicit offline Node writer contract test and three original milestone/actor
regressions. Formatting, scoped Clippy, workspace check, architecture/legacy guards
and 22 Python tests passed. The platform-only filesystem target is gated to the
same macOS/arm64 qualification surface. Full all-target Clippy remains outside
this scoped claim because of the preserved unrelated ST04 peer lint. All 17
protected peer/tracker files remain byte-identical.

Independent final review closed clean for exactly one inert pinned version-only
probe. It passed in 2.13 seconds with exact version, process-bound READY marker,
original Code(0), complete physical EOFs and no deadline expiration. Original
stdout was 22 bytes and stderr was empty. Proxy accepted/connect counts were
zero; source/vendor/ICU bookends, canaries, quiet listeners and bounded scan all
passed before assertions. The owned successful root was removed after custody.
This establishes preload execution only; no TLS observer or fallback is qualified.
Kim's configured GLM model and Bash policy were rechecked; the unchanged
broad allowlist prevented write preflight. Native Sol low implemented the bounded
probe and a dedicated Luna medium writer supplied regressions, with primary
integration and final verification. No Vibe task/configuration change followed.
No credentials, provider calls, production admission, install, selected pin change,
push or release is selected by this work.

## Reviewed client-error observer boundary

The approved follow-on diagnostic observes only the first four global fetch calls.
A function Proxy forwards original arguments, receiver and properties, including
`preconnect`; the native function is invoked exactly once. Synchronous throws and
promise rejections preserve the original error object. Observer failures cannot
replace native outcomes. Promise identity, timing and unhandled-rejection behavior
can change, so this is an instrumented diagnostic rather than stock-runtime
qualification. Native paths outside global fetch may remain unobserved.

Only bounded own data-property `code` and one own data-property `cause.code` are
mapped through a fixed allowlist. Getters, coercions, messages, stacks, request
arguments and response data are excluded. Outer and cause categories remain
separate; missing, accessor and unknown values are explicit. Separate nonce/PID
metadata never enters the original CLI certificate classifier.

Distinct version and negative metadata files each retain the existing 512-byte
limit. The new mode reserves these 1024 bytes from the original 4096-byte helper
metadata budget, leaving 3072 for the unchanged milestone helper. It preserves
four total/two concurrent connections, network bounds, profile and deadlines.
The helper's 25 seconds begin at its original spawn; observer overhead cannot
restart or extend it. A missing healthy version completion blocks the negative
run. Missing final metadata after termination permits only an incomplete
diagnostic prefix, never observer qualification. The original CLI refusal and
per-session certificate-alert gates remain mandatory.

The final recorder-health field covers observation through the final append; it
is not an independent witness that its subsequent close succeeded. Original-child
exit settles process-owned descriptors. Overflow reports calls outside the first
four-call observation window and cannot establish coverage of those calls.
An incomplete prefix has no known total call count; its default count is not
evidence that no fetch calls occurred.

## Single instrumented measurement

Implementation: `3fdb29e`. On 2026-11-06 the separately gated release-mode
`fake_only_client_reason` measurement failed in 18.33 seconds. The instrumented
version phase passed: exact original 22-byte version output, empty stderr,
Code(0), complete observation/EOFs and a healthy 94-byte metadata completion with
zero version fetch calls. This was part of the one measurement, not another inert
support probe.

The metadata-only measurement log has SHA-256
`e974e722a0893580e2781991c718a05620165ce59d3f92bf974c7c418d4187fd`.

The negative original PID 82743, start 1793960887.690838, wrote a valid 43-byte
READY prefix bound to its retained identity and fresh nonce. It wrote no error,
overflow or final record. It reached the original 15-second observation deadline
with empty original stdout/stderr and no observed exit; retained cleanup settled
the original with Signal(9). Missing completion leaves the total fetch-call count
and observer health unknown. It does not prove that fetch was unused, that no TLS
error occurred, or that callbacks were suppressed. Native preconnect/other paths,
unsettled observed promises and other coverage limitations remain unresolved.

All four admitted sessions again reported handshake/TransportEof after native
server Certificate/ServerHelloDone write milestones, without observed client key
exchange, CCS, Finished or alert. Each forwarded 1482 bytes toward the helper and
1132 toward the client, with complete draining, EOF/half-close and owned socket
closure. Peak active sessions remained two. A fifth accept was rejected as
ExcessConnection at 2,720,536 microseconds; it was not admitted upstream. The proxy
joined with four retired slots and all owned sockets closed. The strict gate
failed at the original negative proxy deadline; original CLI refusal, per-session
certificate-alert and observer-completion predicates remain unsatisfied.

The original helper PID 82618 exited Code(0), with complete observation, both
physical EOFs, 479 bytes stdout and empty stderr. Source/vendor/ICU bookends,
canaries, denied IPv4/IPv6 listeners and bounded fake-token scan passed before
failure assertions. The failed root and outside canary remain retained:
`harflow-os-measurement-43d21140385c7f814a864b58404acef1`.

No request contents, raw error text, credentials or provider calls were captured.
No fallback, rerun, broader observer, gate relaxation or production admission
followed. The closed category observer is not proven to cover the failing native
TLS path. A future observation would first need a separately reviewed way to
distinguish hook invocation from pending completion within the same budget.

The separately authorized [hook-entry and settlement stage](claude-client-hook-phases.md)
adds bounded phase markers; its receipt records its own outcome and does not
retroactively qualify this measurement.

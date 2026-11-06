# Client-side TLS reason: preload observability qualification

Status: inert support probe implemented; final checks and independent source
review passed. No pinned-Claude probe or TLS request has run in this stage.
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
probe. That probe is pending. No TLS observer or fallback is qualified by these
checks. Kim's configured GLM model and Bash policy were rechecked; the unchanged
broad allowlist prevented write preflight. Native Sol low implemented the bounded
probe and a dedicated Luna medium writer supplied regressions, with primary
integration and final verification. No Vibe task/configuration change followed.
No credentials, provider calls, production admission, install, selected pin change,
push or release is selected by this work.

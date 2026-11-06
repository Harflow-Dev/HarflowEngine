# ST04 fixed native helper: Q0 review candidate

This infrastructure is a diagnostic adapter for Python-owned ST04 readiness.
HarflowMCP remains canonical; it neither adopts ST04 in Rust nor admits a public
runtime, backend, replacement or workflow certificate. Q1-Q3 remain incomplete.
No helper execution is authorized until the primary has reviewed Q0.

## Reproducible manifest schema

Before execution record one `st04-helper-manifest-v1` document outside product
roots, with mandatory SHA-256 fields (64 lower-case hex): `helper_source_sha256`,
`helper_binary_sha256`, `compiler_binary_sha256`, `profile_bytes_sha256`,
`controller_source_sha256`, `build_invocation_sha256`; plus exact compiler version,
repository HEAD and dirty patch digest, Darwin release/build/architecture/boot UUID,
root device/inode, physical helper path/device/inode/mode, argv and empty env.
Hash exact UTF-8 source/profile/invocation bytes, without newline normalization.
Record the build invocation as a JSON array, including absolute source/output
paths. Build only with `/usr/bin/clang -std=c11 -Wall -Wextra -Werror -O0 SOURCE
-o OUTPUT`, offline, into the fresh fixture runtime. The source is embedded as
`HELPER_SOURCE`; `BUILD_ARGUMENTS` pins its options. No automatic build/execute
entry point exists. The controller must verify the manifest before issuance.
Serialized manifests are provenance diagnostics; there is no admission token or
JSON-to-authority constructor. The adapter compares binary bytes before issuance,
but does not close the executable pathname TOCTOU gap. Thus immutable build closure
and comprehensive effect absence remain unsupported, regardless of report text.

## Protocol and finite envelope

One original retained direct child, one controller/main thread, zero helper threads and at most one sequential fixed probe child, three inherited stdio pipes; native spawn's existing
CLOEXEC_DEFAULT allowlist is required. Helper checks FDs 3..255 but that alone
cannot exclude higher descriptors. Independent descriptor injection controls are
required. No guardian, storage writer, detached descendant or async reader is
created here. Source includes closed fork/posix_spawn, read/write, network and signal probes,
with no arbitrary delegated program. Fork children immediately _exit(0); spawn
selects only the same helper's --child mode. The original C parent synchronously
waits each exact child before continuing, records its PID and exact wait status,
and never runs an owner-death cut. These reports are not independent generic
child identity evidence; broad descendant qualification remains unsupported.

Helper receives exactly five arguments: write path, read witness, controller's
owned loopback port, expected parent PID, and check/signal mode. It checks its
actual parent PID before any probe. Helper emits exactly `ST04-READY` plus newline,
then polls stdin for at most 4000ms.
Controller waits for that exact 11-byte barrier and sends exactly one `R` byte,
permanently retiring issuance even if delivery is uncertain. Helper tries one exclusive/no-follow create, then read, fork, spawn, connect
and signal permission probes. Each emits one exact bounded result line; successful
fixed child controls also emit PID/wait lines. Successful network connect writes
exactly `ST04-NETWORK` plus newline; controller must independently read that marker
on its original exclusive listener. Unexpected errors fail the helper and are
never labelled allowed. Write control writes exactly 12 bytes, read witness is
18 bytes. It emits a separate stderr sentinel and exits. No retries. Capture is capped at 4096 bytes per stream,
8192 bytes per pump step. Launch uses the existing 2s deadline; release uses 4s;
settlement uses 10s. Helper executable is at most 1MiB. Topology uses existing
FixtureRoot limits: 1024 entries, depth16, 16MiB per file. These are fixture
budgets, not production shutdown guarantees. Spawn/compiler/filesystem calls
remain nonpreemptible; no universal wall-clock completion claim is made.

## Sandbox and evidence

The distinct `st04-fixed-absence-v1` profile defaults deny, permits process-exec
only for the physical helper, permits dyld reads beneath `/System/Library` and
`/usr/lib` plus the exact helper and `/`, and denies file-write*, process-fork,
network*, signal, mach-lookup and appleevent-send. It has no workspace write
grant and no env wrapper. Metadata reads are broad and must be recorded. System
read closure, Seatbelt behavior and descriptor closure require actual host
qualification. No settings or provider flags are accepted.

After successful launch, original `RetainedOwnedChild` identity/wait authority
and `FixtureRoot` are held through all observation errors. Preparation or launch
errors may consume the typed handle while retaining the physical root. A missing probe is checked independently using symlink_metadata after positive
reap and both-stream EOF. The original loopback listener witnesses absence or
receives the exact positive marker. Witness bytes and topology are rechecked.
Capture errors, overflow, byte mismatch, missing release/exit/EOF or unknown
probe state prevent `settled`. `absence_qualified` is always false: helper
assertions and filesystem absence do not establish no fork/network/delegation.

`ProbeControl` selects a separately rendered single-permission positive control:
Write grants only the exact probe path; Read the exact witness; Fork permits
the shared process-fork kernel permission (both fork and posix_spawn are exercised;
spawn still selects only the same fixed helper); Network grants only the
original observer's loopback port; Signal permits signal. Controls remain
non-absence diagnostics (`settled == false`); tests inspect independent effect
witnesses and exact bounded raw result lines. Network permission unknown/error
is never success. No helper or control has yet been executed at Q0 source stage.

Default signal mode uses kill(parent,0), a permission check rather than an actual
signal. `enable_controller_signal_probe` opts into harmless SIGCONT only when
the caller executable is the dedicated `st04_helpers-*` fixture test binary,
observes and pins its ProcessIdentity, rechecks it before launch, and supplies its
expected PID to the helper. No arbitrary calling application may opt in. Record
the original controller's identity/mode in the manifest. Production malicious
caller authentication is outside the trusted isolated-fixture threat model.

Q1 measured results and old/successor exact-profile repeats require primary Q0
approval first. Q2 writer closure and Q3 Python workflow composition reuse existing
owner/writer infrastructure; this adapter creates no storage writer, workflow
certificate or Rust contract adoption. Generic descendants, owner-death while a
control child is active, storage faults and universal shutdown remain unsupported.

## Cleanup

No Drop implementation deletes roots or signals actors. On error callers retain
the measurement, original child and root; root paths persist even if the object
is dropped. Stop authority is only the retained child's identity-checked signal
API. No PID lookup or broad kill is permitted. Teardown is external and explicit,
after original child reap, both EOF, all other independently enrolled reader,
writer, FIFO, callback and claim obligations and topology checks. There is no
cleanup-success token in this slice; `settled` covers only this direct child and
capture. Unknown cleanup retains artifacts/claims and fails the case.

Before consuming `prepare` or `launch`, the caller must durably record the fresh root path/identity and manifest. Errors retain the physical root but may consume the typed handle; that is never cleanup success.

Use `HelperLaunch::spawn_spec()` after controller-mode selection to record exact effective executable, captured cwd, argv, empty environment and capture bound before issuance. Launch reuses that accessor; callers must not reconstruct port or argv from profile text.

Signal probes strictly parse and pin the original expected parent PID, recheck actual parent immediately before signalling, and send only to the pinned target. They never refresh a target after reparenting. Controller-death cuts are prohibited for SIGCONT controls; original process identity races remain outside this trusted fixture qualification and do not create production signal authority.

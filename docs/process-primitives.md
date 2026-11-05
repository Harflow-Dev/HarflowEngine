# Internal process primitives (R03-A)

This seam controls isolated owned fixture children on macOS arm64. It is not a
detached owner, execution engine or public run/resume/cancel API. The scaffold
remains unsupported. R02 fixture storage and reads do not establish execution
readiness; no process state is ingested into storage here.

## Identity and control authority

A process identity consists of a positive PID greater than1, start seconds and
microseconds, and the kernel boot-session UUID. The adapter reads the boot UUID
before and after fixed BSDInfo observation, validates the returned PID/start
fields and UUID shape, and refuses an inconsistent observation. Generic probe
failure is Unknown, including a process that disappeared or cannot be inspected;
it is not converted into proof of death. A retained child's observed exit supplies
separate death evidence. Injected observations are not production OS evidence.

A retained std Child is the sole waiter. Its PID cannot be reused while it remains
an unreaped child. All wait/control methods require exclusive mutable access;
there must be no external waitpid(-1) loop, SIGCHLD auto-reaper or other code
reaping these children. Before a numeric signal, the expected identity must equal
the registered identity and owned PID, a fresh injected identity observation must
match, and the deadline must remain live. A successful reap permanently disables
numeric signaling, independently of whether exit decoding succeeded. Stale, other
PID, mismatched boot/start and unknown observations refuse. There is no bare
nonchild PID or PGID signaling API.

The adapter retains ownership when identity or pipe setup is uncertain. Failed
rendezvous or missing acknowledgment does not mean no child ran, and cannot
justify a retry. The caller must keep that handle and arrange explicit qualified
or cooperative cleanup. Dropping closes pipes; it does not send a signal. That
closure may trigger cooperative EOF handling or ordinary pipe errors in the child;
it is not proof of cleanup or permission to release reservations.

## Spawn and fixture rendezvous

The executable and cwd are absolute paths; arguments and environment have explicit
count/byte bounds, and the inherited environment is cleared. Stdin/stdout/stderr
are private pipes. std CommandExt::process_group(0) creates the initial dedicated
process group without a workspace unsafe pre_exec hook. It does not establish
session isolation or containment of arbitrary descendants.

The library supplies bounded private stdin writes and captured raw bytes. Tests
compose a bounded nonce echo and readiness/exit barriers through those pipes.
There is deliberately no engine registration protocol, detached owner executable,
secret transport, durable reservation or acceptance state machine here. A future
R03 owner must bind prepared request/store generation/claim identity and reconcile
lost acknowledgments under the R01 protocol before reporting launch acceptance.

Spawn and OS observations are synchronous OS calls and cannot be preempted inside
the syscall. Deadline checks prevent known-expired admission and later scheduled
effects; they are not a hard wall-clock bound on an arbitrary filesystem/kernel
operation. No background action or retry survives a returned refusal.

## Capture, exit and drain

Both output pipes are nonblocking. Each pump has a bounded byte budget split
between stdout and stderr, deadline checks around reads/interruption and a bounded
poll wait. Each retained stream buffer is capped independently; overflow is
reported while excess bytes are drained/discarded. Raw bytes, including incomplete
UTF8, remain raw. There is no lossy decoder or database work on the drain path.
No blocked background reader thread can outlive an escaped pipe holder.

Integer exit, signal termination and unobserved exit are separate values. Each
stream carries its own EOF, overflow and read-error observations. Exit does not
imply EOF/drained, and EOF does not imply lossless capture: overflow can coexist
with a fully drained pipe. An inherited pipe holder can keep EOF absent after the
direct child exits. Observation cancellation stops observation only; signaling is
explicit. TERM, a bounded grace observation, fresh qualification and KILL are
separate primitives composed in fixtures, followed by bounded exit/drain observation.
No single primitive advertises complete cancellation.

## Containment and platform limits

Containment remains Unknown. Group/tree cancellation is withheld: a matching
leader does not qualify every member or a session-escaped descendant. No saved
PGID is signaled after leader exit/reaping. Process-group creation is not an
escaped-tree guarantee. An exited direct child with a surviving/unknown descendant
or held pipe cannot prove cleanup and must not release future reservations.

Tests retain independent private shutdown channels for descendant fixtures and
verify cleanup using parent-owned observations. These channels are fixture safety
mechanisms, not a production descendant controller. Reboot and PID reuse injection
are explicitly modeled; real reboot/PID-reuse, owner-death recovery, power loss,
performance and Linux/Windows/x86 qualification remain separate. The current
sandbox may refuse boot/process observations; that must produce Unknown/refusal,
not a fallback identity. Empirical macOS checks run with specifically authorized
fixture-process access and are reported separately.

## Reviewed dependency boundary

Only hf-platform gains these exact normal macOS-target dependencies; defaults are
disabled and no optional, renamed, dev/build or expanded feature declaration is
accepted by repository policy:

| Crate | Features | Provenance / license |
| --- | --- | --- |
| libproc0.14.11 | none | [source package](https://crates.io/crates/libproc/0.14.11), MIT, declared MSRV1.72 |
| sysctl0.7.1 | none | [source package](https://crates.io/crates/sysctl/0.7.1), MIT, edition2024 |
| nix0.31.3 | fs,poll,process,signal | [source package](https://crates.io/crates/nix/0.31.3), MIT, declared MSRV1.69 |

That table records R03-A's original boundary. [R03-B2](owner-handoff.md) narrowly
adds nix's socket feature for deadline-aware nonblocking fixture gate connections;
its receipt records the additional locked closure and qualification limits.

Workspace source remains unsafe-forbidden. These wrappers contain dependency
internal unsafe and are not a proof of whole-program memory safety. Only fixed
libproc BSDInfo and sysctl boot UUID string access are used; generic value_as<T>
and custom PIDInfo implementations are excluded. libproc's wrapper accepts any
positive returned size; field validation does not independently prove the entire
kernel structure was returned. Observation failures remain unknown.

The locked closure adds32 packages, including libproc's build-time bindgen0.72.1,
clang-sys1.9.1/libloading0.8.9, libc0.2.191 and sysctl's enum-as-inner/thiserror
procedural macros. These do not add domain dependencies or serialization macros.
libproc generates bindings using the installed Clang/SDK; local build succeeded
with Apple clang21.0.0 (clang-2100.1.1.101). That build dependency is not a runtime
Clang dependency or an independent packaged artifact guarantee. New package
metadata licenses are MIT/Apache alternatives, BSD-3-Clause, ISC or Unlicense/MIT;
the exact registry checksums and versions are pinned in Cargo.lock. No global
library/toolchain installation or source unsafe exception is introduced.

See the [R03-A receipt](migration-receipts/R03-A.md) for exact APIs, empirical and
modeled evidence, final checks and the next ownership prerequisites.

## Current API bounds

SpawnSpec allows at most128 arguments and128 environment entries,65536 aggregate
argument/environment bytes,4096 bytes per executable/cwd path and1048576 retained
bytes per stream. Input counts are checked before iteration; size arithmetic is
checked. Private stdin accepts at most65536 bytes per call and returns the accepted
count so callers retain unsent bytes. Each pump reads at most65536 bytes total,
shared between streams, and polls at most20ms before fresh deadline checks. The
20ms value bounds a poll wait, not a whole pump invocation. Interrupted reads
retry with a fresh deadline check; no fixed syscall-attempt count is promised.

SpawnInitialization distinguishes Ready, UnknownIdentity, PipeSetupFailed and a
post-spawn DeadlineExpired. Ready means only primitive initialization, not owner
acceptance or a completed rendezvous. An initially unknown registered identity is
not rebound by this slice; such a child requires cooperative cleanup or a later
explicitly designed ownership-recovery path. A fresh identity alone cannot override
the missing original binding. Previously registered children may be freshly
qualified for explicit cleanup. No signal outcome proves subsequent exit/drain.

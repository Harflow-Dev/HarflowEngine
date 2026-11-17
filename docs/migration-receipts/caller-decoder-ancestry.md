# Caller record decoder and shared ancestry checks

Date: 2026-11-17. Base: `ce4f1935c1e782601d52acfc8437cce432a4f775`.
Status: internal source and deterministic evidence delivered, not caller/runtime
adoption or R03/R14 acceptance.

## Decoder

`decode_caller_record` requires explicit `MacOsArm64Sdk26_5` selection and exactly
648 bytes. Checked little-endian decoding validates signed PID/parent, positive
seconds, microseconds 0–999999 and status 1–4. The selected fields are seconds at
0, microseconds at 8, status at 36, PID at 40, effective UID at 420 and actual parent
at 560. Padding and alternate real/saved UID or ptrace-parent fields are ignored.
The decoder allocates no kernel-sized buffer and introduces no unsafe code.

The layout evidence was separately measured with public SDK 26.5 on macOS 26.6.2
arm64. Selecting that layout does not admit another OS/SDK/architecture or a
production process reader. The external sysctl fork candidate is not adopted.

## One ancestry algorithm

The existing `observe_caller` now delegates to the same injected evidence-source
algorithm used by deterministic tests. Its private system source retains libproc,
sysctl boot observations and actual `getuid`; PID 1 acquisition still refuses on
the observed machine. Invalid caller PIDs refuse before evidence queries.

The core retains the 128 non-init-link cap, cycles and complete-chain refusal,
caller-only real-UID equality, root-owned or differing-UID ancestors, raw boot
bookends and exact link/init re-observation. Init remains distinct from a normal
`ProcessIdentity`. Status is validated on each read; valid scheduling transitions
remain allowed. One deadline brackets every boot/record provider call, and late
results stop subsequent evidence reads. At 128 links the maximum is 258 record
and 516 boot provider calls, without a kernel-syscall-count claim.

Socket generation remains `Uncorrelated`; cancellation authority and public
capabilities are unchanged. No Cargo dependency, policy or host admission change
was made.

## Verification

Nineteen new integration tests passed: five decoder and fourteen ancestry tests,
with additional internal mutation/boundary matrices. Coverage includes signed and
length boundaries, alternate offsets and padding; complete and truncated chains;
UID, boot, PID, parent and generation changes; re-observation errors; all 24
before/after read-expiry positions for a one-link chain; and the exact maximum
provider counts. Fixture mutations assert they actually change their baseline,
and failed reads are checked against full expected call traces.

The final combined workspace run passed 2,319 tests, with 65 existing ignores and
no failures. Workspace check, strict all-targets Clippy, formatting, architecture,
legacy checks and the default scaffold smoke check passed. The documented Python
verification ran 951 tests successfully, with two skips and three expected failures. Independent caller
source review cleared the implementation. A pre-existing capture-fixture batching
assumption found by the broader run was corrected separately, preserving all
assertions; see [fixture receipt](capture-fixture-priming.md).

Full live ancestry, boot/runtime compatibility qualification, adoption of an exact
immutable fork commit and matching lock/policy changes, and normal fake-backed
task success remain open. No real provider, desktop, publication, installation,
push or live user-state operation occurred. External plans, review records and
verification logs are in the parent workspace under
`.claude-work/codex-2026-11-17/next-mac-rust/`.

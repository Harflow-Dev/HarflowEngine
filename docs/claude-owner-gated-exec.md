# Claude owner gated-exec diagnostic

Status: implemented and independently reviewed; the single bounded attempt on
2026-11-06 refused before owner creation. No owner-hosted vendor qualification is
established. This receipt concerns only the owner-authorized local scripted
provider experiment described in [the decision](decisions/isolated-claude-owner-qualification.md).
HarflowMCP remains the canonical engine and public contract owner. Production
admission, supported/candidate pins and client runtime selection remain unchanged.

## Private composition

The closed `HFCQ1` preparation binds the original caller, request, source, ledger,
recorded root roles, scaffold/vendor file pins, exact generated sandbox bytes,
nonce/port and reconstructed no-tools command/environment. The old `HFPL1` helper
route and its budgets retain their existing meaning. Qualification modes reject
other preparation kinds before preparation/bootstrap mutation.

The owner claims once before spawning the fresh source-owned bridge. It durably
records that original child, establishes capture, then sends Start. The vendor
image is reached only after the bridge receives complete Start plus EOF and
validates exact current store/fence/child/source bindings. It emits request-bound `LaunchReady`, restores blocking
stdio and execs the fixed sandbox/env/vendor chain without forking. `LaunchReady`
is bridge acceptance, not proof of successful exec or protocol completion.

The exec barrier rejects active/quarantined leases and foreign descriptors. Known
identity-checked dormant lease descriptors must be CLOEXEC; stdio must be private
FIFO descriptors without CLOEXEC. The registry lock stays held through exec. The
fixed witness first acknowledges complete Start before waiting for EOF; a distinct
second identity frame comes only from the postexec image. The test checks original
PID/start/boot identity, exact CWD, empty environment and blocking stdio-only inheritance.
No generic entry guard or lease
eviction rule is relaxed.

The fake provider holds its validated messages POST before fixed SSE delivery.
At that point the harness compares the stored original child identity with a
fresh kernel executable-path observation, then releases the response. The live
private final receipt, exact current source/raw extent and original owner byte
commitment must agree before the strict existing protocol oracle is applied.

Owner state and raw/ledger/control paths are outside vendor-writable workspace
and runtime roles. The same-profile preflight checks owner-control write denial,
inside/outside reads, fork/posix_spawn denial and allowed/denied loopback routes.
The exact `hw.pagesize_compat` read is the sole additional sysctl allowance from
the previously measured compatibility profile. No tools, real credentials,
paid inference or external endpoint is used.

## Bounds and diagnostic limits

The safety preflight and version phase have 5/15-second observation limits before
owner creation. Vendor work has a 90-second observation limit within the owner's
100-second work envelope; writer input has its own defensive 100-second cap. The
controller observation is capped at 130 seconds. On expiry no successful receipt
is issued. Original handles and root evidence remain retained until positive
settlement; these numbers do not promise bounded teardown of arbitrary OS I/O.
The owner/writer are never signalled. Only the retained original diagnostic vendor
may receive its identity-checked deadline signal.

Disk-path/hash bookends and a kernel executable path do not atomically prove mapped
image bytes. The explicitly selected physical Claude target is qualified only for
this diagnostic composition, not its logical symlink, complete runtime closure,
timezone correctness, another platform, authenticated service, tools/live input,
arbitrary caller grants or production containment/resource guarantees.

## Verification record

The final frozen-source review closed in round three with no blocking findings.
The minor attempt-ID log label was corrected before the actual experiment.
Locked local workspace tests passed **421**, with **zero failures** and **24 ignored**
measurements (including concurrent ST04 work, not attributed to this change).
HFCQ codec tests passed 5/5; fixed exec witnesses passed 3/3. Formatting, workspace
check, architecture/legacy-name guards, 22 Python tests, all-target Clippy excluding
hf-platform plus hf-platform library Clippy passed. The default executable still
reports unsupported engine operations. Full all-target Clippy remains blocked by
concurrent ST04 test `tests/st04_helpers/support/case.rs:209` (`unbuffered_bytes`);
its owner was notified and those files were left untouched. No hosted CI result
is asserted.

### Actual single attempt

The exact ignored `installed_claude_owner_hosted_scripted_protocol` test ran once:
one failed measurement, zero passed, 18 filtered out, **15.24 seconds**. No retry,
deadline enlargement, hash/pin change or security exception followed it.

The controlled spawn positive and same-profile no-fork/read/network/owner-control
write-denial preflight passed. Only the nonce-bound preflight GET reached the
provider. The pinned physical Claude `--version` child (PID 2765, start
1793934555.496110, boot `0ed817ca-bca2-4d50-a012-e83a12e36b04`) reported
`2.1.318 (Claude Code)` with original wait result Code(0), both EOFs, no overflow or
read error, and empty stderr. Its 22-byte stdout SHA-256 was
`9d30587ab1d5620e3b0a96177dc1d5a79c78ccc2518e714823babbd7636efb3e`.
The physical vendor path/length/hash had matched the closed pin before that child.

The subsequent vendor file-pin observation returned `DeadlineExceeded` at the
unchanged shared 15-second version-phase limit. The post-version hash bookend did
not complete, so the phase was refused despite the successful version child. The
cause and cost distribution within the phase were not measured. No schema store,
qualification parent, owner, writer, bridge, protocol POST, capture receipt or live
vendor-path observation was created. This does not exercise the real gated-exec
composition. The provider worker joined with `Ok(PreflightOnly)`; IPv6 and denied
port witnesses were both quiet. Canary and owner-control bytes/identities remained
unchanged. No SIGTRAP was observed in this version child.

Retained evidence root (0700, device 16777230, inode 603143166):
`/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-7205393ccda8e63ce291202ce558c572`.
The outside canary was also retained. Post-attempt read-only inspection confirmed
that `store` does not exist. Failed qualification evidence was not deleted.

| Binding | Observation |
| --- | --- |
| Host | macOS 26.6.2 / 25G83, arm64 |
| Nonce / exclusive loopback port | `7205393ccda8e63ce291202ce558c572` / `63100` |
| Vendor | physical `/Users/nayanda/.local/share/claude/versions/2.1.318`, 319598464 bytes, SHA-256 `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d` |
| Base profile SHA-256 | `ddd3aca4b152165bd68b7effa09bbcf99cf5f6066f3c67d8eb0f62c6bd4bbea8` |
| Selected profile SHA-256 | `839b1cdd90deb3473fc5d2217443cbfe9763f167cdd217c5688adf4ce3f70d66` |
| Probe log | `/private/tmp/harflow-gated-claude/owner-probe.log` |
| Probe log SHA-256 | `c06dd2f78e1793a1a424fc697360a4699d6506bcd18790c9f41c76bdc12549de` |

A fresh experiment needs review of bounded hashing cost and phase accounting first.
The current deadline and refusal remain intact; no successful owner-hosted receipt,
production admission, push or release is claimed.
An independent fixed exec-witness run observed one unexplained no-EOF timeout at
six seconds. The revised readiness handshake passed all three witness tests in an
independent run (4.45 seconds); this does not establish the earlier failure's cause.
No assertion or deadline was weakened in response.

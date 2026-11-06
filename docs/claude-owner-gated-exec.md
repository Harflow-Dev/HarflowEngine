# Claude owner gated-exec diagnostic

Status: the optimized local owner-hosted scripted-provider experiment passed on
2026-11-06. The earlier debug attempt refused before owner creation and remains
recorded below. This qualifies only the exact isolated diagnostic composition. This receipt concerns only the owner-authorized local scripted
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

### Earlier debug attempt

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

The earlier refusal is preserved. A later owner-authorized optimized attempt is
recorded below; its success does not retroactively complete this failed attempt.
An independent fixed exec-witness run observed one unexplained no-EOF timeout at
six seconds. The revised readiness handshake passed all three witness tests in an
independent run (4.45 seconds); this does not establish the earlier failure's cause.
No assertion or deadline was weakened in response.

## Optimized local continuation

The owner subsequently authorized real installed Codex, Claude, Antigravity, Cursor
and Vibe experiments, including existing account credentials and billable calls.
That permission is recorded; this particular attempt still used only the closed
fake-provider assignment. It used no real credential or paid call. Existing
no-tools/no-fork isolation, production refusal and the no-push/no-release boundary
remain intact. No engine source, dependency, pin, profile grant or deadline changed.

### Diagnosis and preparation

A temporary read-only harness called the same bounded vendor observer. After all
builds finished, its debug-linked single-vendor observation reached
`DeadlineExceeded` at 15.000315292 seconds. The fully optimized dependencies
(`sha2`, hf-storage and hf-platform at opt3 rather than opt0) completed a
representative four-file hash workload in **2.827683 seconds**: ICU-before
0.164807, vendor-before 1.162788, vendor-after 1.357457 and ICU-after 0.142477.
All expected hashes matched. The ICU timing used the generic binary observer,
while the actual harness retains its dedicated ICU observer. This workload
represents hashing cost, not the full version phase; it excludes platform/version
commands and cannot recover the earlier run's unlogged phase durations.
An initial debug timing overlapped compilation and was discarded as confounded.

Two independent read-only diagnoses supported cumulative debug hashing cost as
the leading explanation. Plan review required optimized dependencies, an explicit
optimized scaffold build, fresh binding of that scaffold pin and optimized fixed
witness tests before another vendor attempt. Those tests passed: digest 3/3,
HFCQ codec 5/5 and fixed exec witnesses 3/3 (4.57 seconds). No debug-assertion-only
security gate or hardcoded debug-profile path was found in the scoped source.
The local Cargo optimized profile is a build mode, not a published release.
The measured scaffold was built in the primary workspace at `ce999b8` with the
preserved concurrent ST04 sources/export still present. Its exact observed binary
hash is recorded below; this is not a clean-commit distribution artifact or a
claim of reproducible publication.

### Actual optimized result

Exactly one fresh optimized ignored owner-hosted test passed: **1 passed, zero
failed, 18 filtered out, 12.19 seconds**. The 5/15/90/100/130-second limits and
all payload/capture/FD bounds stayed unchanged. Per-instance root/port profile
bytes and the optimized scaffold pin were freshly observed and bound into the
configuration/request; security grants and the physical vendor pin stayed fixed.

Same-profile controlled-spawn positive, read/fork/posix_spawn/network/owner-control
write-denial preflight and version pin bookends passed before owner creation.
The private parent was PID 85007/start1793935999.555383; the owner was
PID 85053/start1793936000.820536. The bridge/vendor child was
**PID 85264/start1793936004.503972**, boot
`0ed817ca-bca2-4d50-a012-e83a12e36b04`. The held validated POST observed that
source-bound original child at kernel executable path
`/Users/nayanda/.local/share/claude/versions/2.1.318`. The final live receipt bound
the same identity, generation `98d9718579424eaaeecf52293d40b084`, attempt
`44444444444444444444444444444444` and current source/fence/raw extent.

The original owner's byte commitment matched the exact current persisted capture:
**4,417 stdout bytes, zero stderr bytes**, both EOFs, no loss and Code(0). The
strict existing oracle required nonce text before exactly one successful terminal
record, with no tools or later activity. The original parent/owner/writer/vendor
wait and channel/reader settlement checks passed before successful cleanup. The
provider worker joined with `Ok(Completed)` after exactly the preflight GET and
one fake-authenticated messages POST. IPv6 and denied-port listeners were quiet;
control sentinel/canary identity and bytes and final vendor/ICU/gate bookends matched.

Post-test read-only inspection confirmed the successful root
`harflow-os-measurement-becca7801684cb267437488bd2c98ee3` and its outside canary
are absent. The earlier failed `7205393ccda8e63ce291202ce558c572` root/canary remain
retained. Successful cleanup does not erase prior refusal evidence.

| Optimized binding | SHA-256 or observation |
| --- | --- |
| Nonce / exclusive loopback port | `becca7801684cb267437488bd2c98ee3` / `51213` |
| Profile | `bb4ca807e5452228196eb9f454b99ebc71717572cf6381dbf84477f55662b76c` |
| Configuration | `6e1f3582d1b7a7fe74698281f0ecf849f69322689dfcc93471c426aef566ae45` |
| Prepared request | `e0793bf6b64f51b75021cdb0666e958b9e8aaf68371e60e5dfd70dc72acac931` |
| Optimized scaffold | `9ecec2cdfb32abd988f63629663a12b0d7bfd6dd20a2b15351b70e74c1b18ae2` |
| Raw stdout | `4484a8fe7764185c61994d8ec8e40a935214595e1215c5107c29750d95b47e7a` |
| Raw stderr | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Framed raw capture | `bd202b583988b0c3f719125e11de62d496c65a601c203a22d49bc29ce2441fd0` |
| Optimized probe log | `/private/tmp/harflow-hash-phase/owner-optimized.log` |
| Optimized probe log hash | `59d77ed571b309b877db208efc0e4f9f7b69ad2dcd14d8db895d48a700032ac5` |
| Representative hash workload log hash | `5856d112e19ce20f6a83f07091c58db50bbc8554a56c388862eb5d984fba1707` |

Independent continuation plan/evidence review closed in two rounds without a
blocking finding. No source fix followed this successful run. Prior local suite
and static results above remain source-identical; concurrent ST04 Clippy caveat
is not converted into a green check. No hosted CI or other backend is qualified.

## Remaining live-provider gate

Live credentials and billing are owner-authorized, but current `HFCQ1` deliberately
reconstructs the exact fake key, private HOME and nonce-bound localhost endpoint.
An environment swap fails semantic validation. Its profile denies user keychain
access/Mach lookup and external egress. A live-provider diagnostic therefore needs
a distinct reviewed authentication/egress composition: secret handling separate
from persisted preparation bytes/logs, minimal explicit credential access, bounded
provider destination/input/cost, the same original ownership/capture/settlement
witnesses and no ambient HOME or tool/fork fallback. Public capability or production
admission still requires canonical HarflowMCP-first contracts and acceptance.
This successful local-provider composition supplies none of those missing proofs.

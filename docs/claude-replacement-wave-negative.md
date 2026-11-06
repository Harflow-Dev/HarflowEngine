# Four-total/two-concurrent fake-only Claude control

Status: implemented; local verification complete. The owner authorized
one installed-vendor measurement on 2026-11-06 under the
[reviewed scope](decisions/claude-transport-replacement-wave.md). It has not run.
No production admission, public capability or selected contract pin changes.

## Implementation boundary

`TwoTunnelProxy::start_replacement_wave` selects a closed four-total/two-active
mode; the original constructor retains two total. Two stable optional slots
prevent retirement from invalidating an index held across admission checks.
Retirement requires genuine EOF, drained buffers and half-closes; original socket
handles and payload/header buffers close and drop before reuse. Preflight closes
separately and is not retained as a payload session. Immutable retired identities,
counters and observations stay bounded to four records and count toward the
original aggregate 256 KiB raw-read allowance per direction. Pending payload stays
at 16 KiB per direction, with one 16 KiB scratch buffer and 8 KiB header per slot.

A fifth total socket or third concurrently active socket poisons globally unread.
IPv6, invalid CONNECT/authority/headers, I/O and deadline/budget failures keep the
same fail-closed behavior. A candidate-limit failure is not proof of TLS
incompatibility. No destination other than the frozen original numeric loopback
helper is available to this mode; no DNS or credential/provider path is added.

The distinct `wave_tls.py` helper shares classification/certificate construction
with the original helper but admits at most four total/two active sessions. It
settles ready sessions before considering listener admission and keeps its fixed
25-second lifetime. It reports only actual session ordinals/peer tuples and closed
category/phase values; unused slots never receive invented verdicts. The strict
codec requires complete unique actual coverage and matches both ordinal and port.
The original wrapper retains its two-session maximum. Neither helper reads HTTP.

The ignored `installed_claude_replacement_wave_tls_negative` entry requires
`HARFLOW_RUN_REPLACEMENT_WAVE_TLS_NEGATIVE=1`. A closed controller mode chooses
the exact helper, proxy and codec; the old gate retains its original mode. Both
paths supply only the literal fake token. No real-credential broker or live phase
is reachable. Original 130-second overall, five-second preflight, 15-second version
and negative observation, and 25-second helper bounds remain unchanged. Cleanup
cannot repair an expired observation or incomplete original-child capture.

The strict certificate-alert/CLI-refusal/all-session gate is unchanged. Numeric
active/pending/aggregate witnesses additionally fail closed. Proxy/helper joins,
original process exit and physical EOF, exact pins, denied listeners, canaries and
bounded post-settlement fake-token scan still precede assertions. Failed roots
remain retained; no raw request, response, TLS bytes, error strings or credentials
are logged.

## Verification and single-run result

Final local verification passed 35 distinct scoped tests: seven wave proxy,
two wave codec, two actual wave helper (including the original 25-second partial
coverage), nine original two-tunnel proxy, six original egress, six original codec,
two local TLS differential and one original helper controls. Both installed-vendor
entries remained ignored during local verification. The helper driver additionally
checks that reader-thread errors set a failure flag without emitting raw errors.

Formatting, scoped Clippy with warnings denied, workspace check, architecture and
legacy guards, and 22 Python tests passed. All 17 protected peer/tracker files are
byte-identical. Workspace all-target Clippy remains outside this scoped claim due
to the preserved unrelated ST04 peer lint. No assertion or production lint was
weakened. Kim's write preflight remained blocked by the unchanged broad command
allowlist; native Sol low implemented the proxy/harness, a dedicated Luna medium
writer supplied regressions, and primary integration completed helper custody.

The independently reviewed single authorized measurement is pending. No R05
acceptance or authentication qualification follows from this fixture.

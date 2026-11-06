# Four-total/two-concurrent fake-only Claude control

Status: implemented; local verification complete. The owner authorized
one installed-vendor measurement on 2026-11-06 under the
[reviewed scope](decisions/claude-transport-replacement-wave.md). It ran once and
failed the unchanged strict gate in 17.20 seconds.
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

Independent final review closed clean before the single authorized measurement.
No R05 acceptance or authentication qualification follows from this fixture.

## Single installed-vendor result

The optimized ignored entry ran exactly once at implementation `d652778` and
failed. The closed mode witness confirms total four/active two. Preflight
isolation, exact Claude/ICU version/hash checks and metadata bookends passed.

| Witness | Observation |
| --- | --- |
| Preflight / accepted IPv4 / validated CONNECT | 1 / 5 / 4 |
| Peak active slots / retired sockets closed | 2 / 4 |
| First global failure | `ExcessConnection`, 3,761,725 microseconds since proxy worker start |
| Helper phase/category for all four sessions | `handshake / transport_eof` |
| Explicit certificate alerts / application data | none / none |
| Each session forward read / forwarded | 1,482 / 1,482 bytes |
| Each session reverse read / forwarded | 1,132 / 1,132 bytes |
| Aggregate raw bytes forward / reverse | 5,928 / 4,528 bytes |
| Peak pending bytes forward / reverse | 1,482 / 1,132 bytes |
| Proxy custody | original worker joined, all sockets closed, all four sessions bidirectionally EOF/drained/half-closed |
| Helper custody | original Code(0), complete stdout/stderr EOF, no capture loss/errors |
| Vendor original observation | Ready, deadline expired, exit unobserved, zero captured bytes, no pipe EOF before cleanup |
| Vendor cleanup exit | identity-bound Signal(9); incomplete observation remains failed |
| Static CLI certificate-error witness / observation within negative deadline | false / false |
| Denied listener / IPv6 attempts | quiet / zero |
| Canaries / bounded post-settlement fake-token scan | unchanged / complete, zero matches |

Recorded helper peers `127.0.0.1:62826`, `:62827`, `:62829`, `:62831` match
proxy upstream-local tuples for ordinals one through four, respectively. The
frozen helper address was `127.0.0.1:62817`. Every admitted session's EOF/drain
and half-close evidence was already established before global poison; retirement
closed the four original sockets. The fifth accepted socket stayed unread and
was not forwarded. Its destination, contents and caller purpose remain unknown.

Allowing one hypothetical replacement wave reproduced the same handshake EOF
outcome. It did not produce a completed CLI certificate refusal or an explicit
helper certificate alert. EOF is not promoted to certificate rejection; the
strict control remains failed. This does not identify why Claude closes, prove
an SDK retry, establish a universal startup connection allowance or qualify the
embedded runtime. No larger cap, longer deadline, revised oracle or automatic
rerun follows. Remaining qualification gates stay in the reviewed design.

All settlement, controls and the bounded scan preceded the assertion. Failed
roots remain retained. All 17 protected peer/tracker files remain byte-identical.
No real credentials, broker, real upstream/provider/model/tool call, paid call,
production admission, install, registration, push or release occurred.

## Retained bindings

- Root: `/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-74112ccae8038fffd3d592c40a2f39a1`.
- Root device/inode/mode: `16777230 / 605183390 / 0700`.
- Proxy port: `62818`; nonce is the root suffix above.
- Profile SHA-256: `10d433e218df3e23bc484792cdcce4ba2f282ae53ea5af3fe902c44110f3b344`.
- Vendor: `/Users/nayanda/.local/share/claude/versions/2.1.318`, 319598464 bytes;
  SHA-256 `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
- ICU SHA-256: `666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549`.
- Log: `/private/tmp/harflow-wave/run.log`;
  SHA-256 `cde29150279e514828b64a081dd710638520a20fe296f9c282504af4660c0eb2`.

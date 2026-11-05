# Backend and OS qualification measurements

This is diagnostic evidence, not production admission. Every stock backend strict
production route remains unsupported. No public execution capability, positive
admission token or conversion from report JSON to trusted qualification is added.
HarflowMCP remains the canonical owner; its specification is pinned at
`a38f643101601e0610d3ad4e54e3bfe6f9742181`, file
`docs/decisions/backend-enforcement-qualification-v1.md`, SHA-256
`968d8e6668a579dcab86f229ea3717505d6d6ef89bbc27cfa24039a9aa419406`.
The existing owner-runtime-v1 and supported 0.0.10/full207 pins remain unchanged.

## What is measured

The macOS arm64 adapter creates fresh physical mode-0700 fixture roots. It accepts
no user tree or asserted exclusivity flag. It records and rechecks the identity of
its root and six role directories; bounded topology inspection rejects aliases,
special files, missing/replaced roles and overflow. This detects some changes and
does not establish race-free production exclusivity against outside actors.

The OS profile denies by default. Writes are limited to the disposable workspace
and separate runtime area, with nested control and Git exclusions. Protected
ancestors and role roots cannot be unlinked/relocated; new hardlinks are denied.
Exact system/helper runtime reads are declared separately. Literal `/` data-read
supports the measured libignition bootstrap; it is not recursive root access.
Literal `/dev/urandom` supports the fixed Python interpreter's entropy startup.
Neither grants general user-data reads. Process inspection, sysctl, Mach lookup,
Apple events and signaling are not broadly enabled to make startup pass.

Safe spawn retains its descriptor allowlist and parent working directory. The
fixed physical sandbox executable launches `env -C` to change only the child's
cwd, then passes the selected logical executable unchanged. Physical executable
hash/metadata bookends are observations, not atomic image identity or an admission
grant. Bounded streaming SHA-256 avoids loading the installed binary into memory;
its maximum input is 512 MiB, separately from production workload budgets.

The profile has two distinct measurement routes: controlled, audited descendants
whose original parents retain their waits; and vendor-no-fork. Controlled helpers
do not qualify arbitrary daemon/setsid containment. The vendor route must prove
both fork and posix_spawn refusal before an installed CLI can start.

## Observed counterexamples and limits

Actual disposable fixtures on macOS 26.6.2 arm64 established that path-only rules
can be bypassed by a pre-existing writable hardlink to a protected outside inode,
and by renaming an allowed ancestor of a protected subtree. These remain preserved
counterexamples, not passing strict configurations. Topology refusal and ancestor
freezing address the constrained fixture layout; arbitrary working trees remain
unqualified.

An already-open writable descriptor can modify a denied file after sandbox entry.
The positive control uses the real non-CLOEXEC descriptor; the safe-spawn control
then observes EBADF for that same descriptor and unchanged outside bytes. Testing
an arbitrary unused descriptor would not establish exclusion.

The nested sandbox route observes an exact apply refusal after a valid unsandboxed
syntax control. It reports unsupported application, not a successful child write
refusal or general nested containment. Helper startup failure, missing paths,
unrelated errno, timeout and signal exits cannot become successful OS denials.

The installed sandbox tool is deprecated. Its existence or profile compilation is
not enforcement evidence; each operation needs controls and external witnesses.
Local measurements do not qualify other OS builds or platforms.

## Conditional installed-Claude fixture

The selected CLI is observed as Claude 2.1.318, executable SHA-256
`677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
This is a distinct private no-tools/no-fork fixture, not the ordinary inherited
configuration route used by Python, and not full R05 qualification.

The factory creates explicit isolated HOME/config/XDG/tmp paths, fake credentials,
no inherited proxies or provider environment, disabled customization/MCP/tool
inputs and a nonce-bound prompt. Managed settings may still affect the CLI;
unsupported startup refuses rather than falling back to weaker execution.

Before any vendor invocation, the same exact profile must demonstrate actual fork
and spawn denial, default denial of an unlisted outside-read canary, an inside-read
positive control and allowed-versus-denied loopback ports. A test-specific canary
deny is deliberately absent. The helper uses isolated Python startup (`-I -S`).

The OS accepts `localhost:<port>`, not the attempted numeric-IP expression. This
permits IPv4 and IPv6 loopback at that port, not IPv4-only confinement. The fixture
exclusively reserves both 127.0.0.1 and ::1 endpoints before profile construction;
HTTP uses the fixed IPv4 URL, and unexpected IPv6 connections fail measurement.
No wildcard listener or port sharing is used. Other ports remain denied.

The server permits one nonce-bound preflight GET, then one bounded messages POST,
with fake authentication, strict HTTP framing and fixed text SSE. It rejects extra
requests, redirects, unknown routes, duplicate lengths and chunked request bodies.
Stream text, terminal protocol result, original child exit, both pipe EOFs and
server-thread settlement remain independent witnesses. No live-provider result is
inferred from a scripted local response.

## Verification and remaining gates

The opt-in OS and installed-CLI tests are separate from ordinary offline tests.
Their actual run results and exact limits are recorded in the stage receipt.
A passing ordinary suite does not qualify an ignored probe.

The measured eight-case OS matrix passed on this host. The selected Claude
executable failed at `--version` with SIGTRAP after the no-fork/read/network
preflight passed; no messages request was reached. A separate version-only
diagnostic added only `kern.osproductversion` read access, repeated the preflight,
and still trapped. Its crash banner gained the OS version, which establishes
reporting access, not the cause of the original trap. Neither profile qualifies
Claude, and no further permissions were broadened to obtain a passing result.

A subsequent unchanged-profile helper diagnosis identifies the captured trap as
WebKit's page-size assertion: `sysconf(_SC_PAGESIZE)` returns -1/EPERM while the
outside-profile positive control returns 16384. The exact binary's assertion
branch corroborates the failure chain. The subsequently authorized exact
`hw.pagesize` version-only candidate still trapped: the modern named query became
allowed, while libc's numeric `hw.pagesize_compat` query remained denied. Its
security preflight passed, child/EOF settlement completed, and the server observed
only the preflight GET. No production policy fix or successful startup is claimed.
The subsequently authorized compatibility-only candidate passed `--version`
with exit zero and the exact pinned version, after its full security preflight.
Only that single compatibility selector was added; modern-name and neighboring
queries remained denied in the independent helper. Child/pipe/server settlement,
profile hashes and declared configuration binding were recorded. This qualifies
only the measured version startup, with no provider or tools session and no
production policy change. The receipt preserves both failures and this result.

See [the measured stage receipt](backend-os-qualification-receipt.md) for the
verification boundary and remaining production gates.

Production caller/grant authority, stable exclusive user-root ownership, complete
runtime/configuration identity, tool-enabled backend behavior, real descendant
containment, production resource envelopes, authenticated providers and other
platforms remain separate gates. Vibe's launcher hash does not identify its full
runtime. Native Claude tool permissions and ordinary Codex inherited rules/MCP
startup are not OS write-confinement proofs. Unsupported routes must say what
qualification is missing; no silent tool-level or unrestricted fallback exists.

No user state, installation, registration, supported contract selection, push or
release is part of this measurement stage.

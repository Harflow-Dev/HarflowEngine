# Backend and OS measurement stage receipt

Status: diagnostic stage; production backend admission remains blocked.
Runtime base: `d2ba1da11b02697d222c58a9f0cc665144832fee`.
Canonical specification: HarflowMCP
`a38f643101601e0610d3ad4e54e3bfe6f9742181`,
`docs/decisions/backend-enforcement-qualification-v1.md`, SHA-256
`968d8e6668a579dcab86f229ea3717505d6d6ef89bbc27cfa24039a9aa419406`.
No supported fixture, engine selection, schema or owner-runtime candidate pin
changes. This receipt is evidence, never executable authority.

## Measured environment and observations

Measurements ran on macOS 26.6.2 (25G83), arm64, Rust 1.99.1.
`/usr/bin/sandbox-exec` SHA-256:
`abc5bb136d6b5cce8fa85d789f78e3326c51ca60cae637b2064adfb67a1dcd9a`.
Claude selected logical path: `/Users/nayanda/.local/bin/claude`, version inventory
2.1.318, physical executable SHA-256:
`677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
The logical symlink argument is preserved; metadata/hash bookends do not establish
atomic executable identity. Other installed CLI inventories are not qualifications.

The final explicit `hf-platform` qualification run passed eight cases: direct
paths, new-hardlink syscall denials with inode/link-count positive controls,
and preserved hardlink counterexample; protected ancestor rename
counterexample plus hardened refusal; fork and posix_spawn denial with positive
controls; controlled direct descendant, shell and alternate executable routes;
nested sandbox explicit apply refusal; and inherited writable descriptor bypass
positive control versus safe-spawn EBADF exclusion. Tests externally inspect
canaries and retain original process ownership. Nested apply refusal is not a
successful nested write-denial claim. Controlled descendants do not qualify
arbitrary vendor daemons or process groups.

Both actual Claude attempts passed controlled posix_spawn and same-profile
vendor-no-fork/read/network preflight. Both then failed at `--version` with
SIGTRAP, reaped original child and both output EOFs. No messages POST or scripted
provider exchange occurred. No authenticated or paid provider was called.

The separate version-only diagnostic added exactly
`(allow sysctl-read (sysctl-name "kern.osproductversion"))`. It filled the Bun
1.4.4 crash banner with macOS 26.6.2 but retained SIGTRAP. That is not evidence
that OS-version denial caused the trap. The base profile remains unchanged;
Mach, process inspection, process creation and broad sysctl permissions were
not relaxed. The startup cause was unknown at stage delivery; the following
diagnosis localizes that observed trap without changing the profile.

### Follow-up: page-size assertion

Two independent read-only diagnoses decoded the existing trace locally; no
telemetry URL was opened. Both runs identify image-relative frame `0x5550377`.
Static inspection of the exact executable identifies the trap at preferred
address `0x105550378`, a `brk #0` carrying `WTF::pageSize()` metadata from
`vendor/WebKit/Source/WTF/wtf/PageBlock.cpp`, line 64. Preceding instructions call
`sysconf(29)` (`_SC_PAGESIZE` in the installed SDK) and branch to this assertion
when the returned value is not a power of two.

The primary independently inspected these instructions and embedded literals,
then ran a fixed C helper from a fresh `FixtureRoot`. The existing
`VendorNoFork` profile factory was used with its declared runtime/helper reads
and an exclusively reserved dual-stack loopback port. No sysctl exception or
other permission was added. The helper invokes no vendor and creates no child.

| Observation | Outside profile positive control | VendorNoFork profile |
| --- | --- | --- |
| `sysconf(_SC_PAGESIZE)` | 16384, errno 0 | -1, errno 1 (EPERM) |
| `sysctlbyname("hw.pagesize")` | 16384, success | -1, errno 1 (EPERM) |

Both helper processes exited zero, were reaped by their original parent and
reached both output EOFs; no listener received a connection. The fixture is
retained as diagnostic evidence. This establishes the observed failure chain:
page-size denial returns an invalid size, which triggers the exact WebKit
assertion in the captured Claude failure. Apple's
[sysconf implementation](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/gen/FreeBSD/sysconf.c)
and [getpagesize implementation](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/gen/FreeBSD/getpagesize.c)
corroborate the call chain. Current upstream sources are explanatory evidence;
the pinned executable and actual helper results identify this local failure.

Artifacts: `/private/tmp/harflow-claude-trap/`, including helper source, generated
profile, disassembly, assertion literals and `result.json`. Result SHA-256:
`7032c02bb11c82e9117c46820f837000a59091d426847e85299dea489de32dbf`.
Profile SHA-256:
`026b5c9650187e6b465f4d62dfe13b58416531d8010994f5740f7403b9e2bf86`.
Helper source SHA-256:
`8be1694f1d9a589d9fd781519a5b594515e088bb7cac60de641d082f8b3891b0`.

No runtime policy was changed and Claude was not rerun in this diagnosis.
An exact `hw.pagesize` read-only exception is the smallest candidate for a later
version-only experiment, with the full safety preflight repeated. It has not
been tested or adopted. Resolving this assertion does not establish that later
startup, protocol, tools or production qualification will succeed.

Exact generated OS profile texts and operation names are captured by the
opt-in probe. Claude logs record root device/inode, base and selected profile
digests, original child identity, phase and bounded failure output. Local raw
logs are under `/private/tmp/harflow-backend-qualification/`; they are transient
diagnostic artifacts, not portable production qualification receipts.

Local artifact SHA-256 values bind the observed runs:

| Artifact | SHA-256 |
| --- | --- |
| `os-matrix-frozen.log` | `b4121a41bf9f0f2d05c567566369ceac5c48f93d1bad6fdaf1e9143a7560756c` |
| `os-profile-bindings.json` (43 operation/profile records) | `e614d023d58886b52cb63452b4db13d29d4861c27acbb02e2f051ce660e10759` |
| `claude-fixture-final.log` | `6ad86ec0035780b6deefe80de4543efbf9d5e0887fdfe76c246810121f9eb737` |
| `claude-osversion-diagnostic.log` | `9f75cf2e768ed14dcd416970c74c09b3ba98e05ccc10652918444f39a2ea11d8` |

## Verification boundary

Ordinary offline verification and the actual opt-in probes are reported
separately. The two installed-Claude probes remain ignored by default and their
explicit runs failed. They are not waived or counted as successful qualification.
The eight OS probes remain opt-in and were explicitly run successfully.
The locked workspace suite passed 362 tests with 13 ignored: three existing
opt-in probes, eight OS measurements and two installed-Claude measurements.
Rust formatting, locked workspace check, all-target Clippy with warnings denied,
architecture/legacy-name/whitespace guards and 22 Python tests passed. The default
scaffold still reports engine operations unsupported.
No hosted CI, Linux, Windows or other macOS build qualification is claimed.

Failed fixture roots are conservatively retained when the full measurement did
not settle, including a successful preflight followed by vendor failure. Earlier
known startup/helper-only roots were removed only after positive original-child
settlement and exact bounded, non-following inventories. Retention does not imply
a live child; it prevents uncertain teardown from being treated as safe cleanup.

## Remaining production gates

Every stock strict backend route returns explicit unsupported with an actionable
missing-qualification reason. No positive admission token or conversion to
`QualificationState::Passed` is exposed.

- Resolve exact installed-Claude startup under the bounded profile using evidence;
  then separately measure the local provider protocol and tool-enabled behavior.
- Establish authoritative caller/grant binding and complete runtime/config identity.
- Establish exclusive production root ownership and stronger alias/race guarantees.
- Qualify real descendant containment and production resource envelopes.
- Measure authenticated provider behavior under separately authorized scope.
- Qualify each additional backend, platform and OS/runtime combination.

This stage does not accept R05, R12 or ST06, adopt steering contracts, or transfer
canonical ownership. Release freeze remains in force: no push, release,
installation, registration or client engine selection change.

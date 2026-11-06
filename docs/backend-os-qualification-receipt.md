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

No runtime policy was changed and Claude was not rerun in that diagnosis.
The recommended exact `hw.pagesize` diagnostic was subsequently authorized and
measured below. No startup, protocol, tools or production qualification follows
from localizing the assertion.

### Exact modern page-size candidate

The owner authorized a diagnostic profile containing only the unchanged base plus
`(allow sysctl-read (sysctl-name "hw.pagesize"))`. This is separate from the
OS-banner candidate and is not a production policy change. Original profiles and
artifacts remain preserved.

The same-root fixed C helper comparison found:

| Operation | Outside positive control | Base | Exact candidate |
| --- | --- | --- | --- |
| `sysconf(_SC_PAGESIZE)` | 16384 | -1/EPERM | -1/EPERM |
| Named `hw.pagesize` (quad buffer) | 16384 | EPERM | 16384 |
| Numeric `{CTL_HW, HW_PAGESIZE}` (int buffer) | 16384 | EPERM | EPERM |
| Named `hw.pagesize_compat` (int buffer) | 16384 | EPERM | EPERM |
| `hw.ncpu`, `kern.osproductversion` | allowed | EPERM | EPERM |

Returned buffer lengths were 8/4/4 for modern/numeric/compatibility page-size
queries. All three original children exited zero, reached both EOFs and were
reaped. No listener received a connection. The helper's successful-sysconf
assertion remained failed; its failure was not waived or counted as a fix.

Apple's [XNU registrations](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/kern_mib.c)
separate modern `hw.pagesize` from numeric `HW_PAGESIZE`, which resolves to
`hw.pagesize_compat`. libc uses the numeric node. Both independent read-only
diagnoses corroborated this distinction. The modern-name allowance therefore
does not satisfy the observed libc page-size requirement.

Artifacts are under `/private/tmp/harflow-claude-pagesize/`; helper `result.json`
SHA-256: `dbe460b43522425d36f96fd5be1531d5a4f6eebe83253c1057bcc4d06bac71e6`.
Helper base/candidate profile SHA-256 values are respectively
`0b87c0bbd8513e25087b319884a5669644094ac529cf95beccd2fdd71f210450` and
`ae02439d89cd24c338eecbbfda6ae08ac7c8dbdbad3350d9d75974e4fe4082a8`.
Source SHA-256: `decaf6a699c08d9c34a9589ac2342cce01fc08f47f55e61adfa0110ba1383fac`.

The existing eight-case OS safety matrix passed again under unchanged policies.
The final candidate run passed controlled spawn and the full existing
no-fork/read/network preflight, then `--version` again exited SIGTRAP. The original
child (PID 25025, boot UUID `0ed817ca-bca2-4d50-a012-e83a12e36b04`, start
1793894005.012817) was reaped with both pipe EOFs and stable pinned binary
bookends. The trace decodes locally to the same `0x5550377` page-size assertion.
Before the unchanged failing version assertion, the harness recorded both
listener witnesses quiet and server outcome `PreflightOnly`; no messages request
or scripted provider response occurred. The failed root is retained.

The final vendor base/candidate profile SHA-256 values are
`c9c9e1953c42dcbf224f5dfc877103b532056c1e57cb8c8818f044586d53923b` and
`fe2df64d24826c28968eeba717a34c0691cedda147ff44056330b1802a68f93d`.
Final `vendor-version-only.log` SHA-256:
`c0f70f99829ea9f0c58006342f77517508ed9d0977991c79d984c416349e421c`.
The earlier pre-review run is preserved separately; final evidence supersedes its
incomplete failure-path settlement reporting. The accepted review fix records
these independent witnesses before enforcing the original failed version oracle.

No provider/model/tools session is authorized. The candidate-specific server
rejects any request except its single nonce preflight GET before sending a
scripted provider response.

At that checkpoint, a separate exact read-only compatibility-selector experiment
required an owner decision. That decision and its bounded result follow; the
modern-name failure above remains historical evidence.

### Exact compatibility page-size candidate

The owner subsequently authorized the separate read-only compatibility selector.
This candidate adds only
`(allow sysctl-read (sysctl-name "hw.pagesize_compat"))` to the unchanged base.
It does not combine the modern-name or OS-banner exceptions. All prior modes,
profiles and failed probes remain preserved as historical evidence.

The independent same-root helper comparison measured base `sysconf`, numeric
`{6,7}` and named compatibility queries as -1/EPERM. With this one exception all
three returned 16384; the modern `hw.pagesize` name, `hw.ncpu` and
`kern.osproductversion` remained denied. The outside-profile positive control
returned 16384. Buffer widths and returned lengths were 8/4/4 for
modern/numeric/compatibility queries. All helper processes exited zero, were
reaped and reached both EOFs; no listener accepted a connection.

The actual installed-Claude run repeated the full existing controlled-spawn and
same-profile no-fork/read/network preflight before invoking only `--version`.
It exited zero with stdout `2.1.318 (Claude Code)` and empty stderr. Original
child PID 38050 (start 1793922107.755278, boot UUID
`0ed817ca-bca2-4d50-a012-e83a12e36b04`) was reaped, both output pipes reached EOF,
and pinned executable bookends matched. Both IPv6 and denied-port witnesses
were quiet; the server explicitly settled as `PreflightOnly`. It permitted no
messages response. The successful fixture and outside canary were removed only
after complete settlement, and fixture absence was independently checked.

This resolves the observed version-startup assertion for the exact measured
binary/platform/profile. It establishes no provider protocol, authenticated or
tool session, descendant behavior or production backend support. The normal
runtime policy, public capabilities, client selection and all supported contract
pins remain unchanged. No further selector was tested or adopted.

Artifacts are under `/private/tmp/harflow-claude-pagesize-compat/`. Exact vendor
base/candidate profile bytes were independently reconstructed and verified
against their logged digests, and preserved separately from older artifacts.
The declared configuration digest uses length-prefixed fields and counts for
logical executable, actual `--version` argv, ordered explicit environment,
source-profile description and workspace CWD. It binds only these declared
inputs; unknown managed external settings are not claimed qualified. An
independent reconstruction matched the logged configuration digest.

| Binding | SHA-256 |
| --- | --- |
| Vendor base profile | `43f7bf899292abf9c6021733cf78ab459456d01139dcd730d38adc542e4b0f14` |
| Vendor compatibility candidate | `e76a600633370c2eb9e1cb0137c8827e38da5e25ac46140b7f20d003fe4559d8` |
| Declared version configuration | `27b12e4aceb09b0f32ccaabcc9ff0262725883543e67379eb7a078c27a98a1e5` |
| `vendor-version-only.log` | `c328874dfe1b89961e2927379c360c03a5f126121b2fa9ffad03238e98b8c723` |
| Helper `result.json` | `d59e92752c82ffc1e946d3bbbcddf40bcbaaa33b36ba943d56bdc5eee1b194e3` |
| Helper base profile | `98d6238a27d93736eb9c99c49c95d2c7cbf34865c4492fb98710dd610842fc86` |
| Helper compatibility profile | `50e7d2e17bb672f3432e2d343aa4c5e072ca0367a86224aaa2716fec41c1a4ff` |
| Helper source | `decaf6a699c08d9c34a9589ac2342cce01fc08f47f55e61adfa0110ba1383fac` |

The repeated eight-case OS safety matrix passed under unchanged policies.
Final ordinary-suite counts are recorded below; a successful version-only probe
never produces `QualificationState::Passed` or strict production admission.


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
separately. The original two installed-Claude probes remain ignored by default
and their explicit runs failed. The additional page-size candidate is also
ignored by default and its explicit run failed, as recorded above. They are not
waived or counted as successful qualification. The fourth, compatibility-only
probe is ignored by default and explicitly passed version startup only.
The eight OS probes remain opt-in and were explicitly run successfully.
The original locked workspace suite passed 362 tests with 13 ignored: three existing
opt-in probes, eight OS measurements and two installed-Claude measurements.
After the candidate and review-driven settlement fix, the final locked suite
passed 365 tests with 14 ignored (the additional version-only probe remains
ignored). Three offline regressions cover exact profile selection and the
preflight-only route guard. The eight explicit OS safety tests passed again.
After the compatibility-only candidate and declared-config regressions, the
final locked workspace suite passed 368 tests with 15 ignored. The focused
host target passed six offline tests; its four installed-Claude probes remained
ignored in the ordinary run. The compatibility probe was separately run and
passed as recorded above. No earlier failed probe was rerun or relabeled.
Rust formatting, locked workspace check, all-target Clippy with warnings denied,
architecture/legacy-name/whitespace guards and 22 Python tests passed. The default
scaffold still reports engine operations unsupported.
No hosted CI, Linux, Windows or other macOS build qualification is claimed.

Failed fixture roots are conservatively retained when the full measurement did
not settle, including a successful preflight followed by vendor failure. Earlier
known startup/helper-only roots were removed only after positive original-child
settlement and exact bounded, non-following inventories. Retention does not imply
a live child; it prevents uncertain teardown from being treated as safe cleanup.

The later [scripted local protocol receipt](claude-scripted-protocol.md) records
the separately authorized bounded exchange, its initialization/wire failures and
final post-fix pass. This does not change any historical outcome above.

## Remaining production gates

Every stock strict backend route returns explicit unsupported with an actionable
missing-qualification reason. No positive admission token or conversion to
`QualificationState::Passed` is exposed.

- Compatibility-only version startup and the separately authorized isolated
  scripted local text protocol are measured. Real-provider authentication and
  tool-enabled behavior remain separate unqualified gates; neither was run.
- Establish authoritative caller/grant binding and complete runtime/config identity.
- Establish exclusive production root ownership and stronger alias/race guarantees.
- Qualify real descendant containment and production resource envelopes.
- Measure authenticated provider behavior under separately authorized scope.
- Qualify each additional backend, platform and OS/runtime combination.

This stage does not accept R05, R12 or ST06, adopt steering contracts, or transfer
canonical ownership. Release freeze remains in force: no push, release,
installation, registration or client engine selection change.

# Isolated Claude scripted protocol measurement

Status: passed for this exact local scripted text exchange; no production admission.
Runtime base: `8254e040f742d76096acf81359ae2e8cbac8d853`.
HarflowMCP remains the canonical engine/contract owner. Supported contract pins,
client selection and every stock strict production backend refusal remain unchanged.
The earlier [backend/OS receipt](backend-os-qualification-receipt.md) retains the
startup failures and compatibility-only version measurement.

## Scope and enforced boundaries

This is Claude 2.1.318 on macOS 26.6.2 (25G83), arm64, using the same fresh-root
no-tools/no-fork profile as the successful version-only measurement. Its only
sysctl exception is exact read-only `hw.pagesize_compat`; neither the modern-name
nor OS-banner exception is added. No filesystem, Mach, process, network or
credential grant was expanded for this stage. The CLI runs with disposable
HOME/configuration, explicit environment, fake fixture credentials, no inherited
proxies, empty tools/MCP inputs and the fixed nonce prompt. Builtin runtime
metadata is not a claim that arbitrary plugins or tool sessions are qualified.

The fixed sandbox/env wrapper, descriptor allowlist and child-only CWD transition
remain in force. The same-profile preflight measures denied fork and posix_spawn,
unlisted outside read, an allowed inside read, and allowed versus denied loopback
ports before any vendor invocation. The fixture exclusively owns both loopback
families at one port. A parent-owned IPv4 listener clone now retains that binding
even if the request worker rejects input while the child is still alive.

Version startup must pass with complete capture and matching binary bookend
before the protocol child launches. Fixed argv input closes stdin before waiting.
The declared protocol digest binds the actual protocol argv/environment/source
profile/CWD, not merely the version argv. Managed external settings remain
unknown; no complete effective-configuration or runtime-closure claim follows.

## Script and independent success witnesses

The local server permits one nonce GET for preflight and then exactly one
fake-authenticated messages POST. It accepts the canonical messages target or
its exact `?beta=true` form used by this pinned CLI. Other queries, duplicate
parameters, paths, count-tokens requests, redirects and additional requests remain
unsupported. HTTP framing, body/request counts and fixed text SSE remain bounded.
Anthropic's [SDK beta messages implementation](https://raw.githubusercontent.com/anthropics/anthropic-sdk-typescript/main/src/resources/beta/messages/messages.ts)
corroborates the beta qualifier on the same endpoint; no fallback destination was
introduced. The server sends a nonce-bound text marker, never a model inference.

The final installed-CLI probe passed all independent witnesses:

- Full security preflight and exact version startup passed.
- One messages POST (1116 bytes) matched the fake fixture key.
- A nonce text witness preceded exactly one success terminal event. Duplicate,
  reversed, post-terminal, failure and tool-bearing streams refuse. Structured
  tool-family types, including MCP and specialized results, are rejected; ordinary
  text mentioning their names is not interpreted as a tool event.
- Protocol child PID 6588, start 1793923975.677237, boot UUID
  `0ed817ca-bca2-4d50-a012-e83a12e36b04`, exited zero and was reaped by its original
  owner. Both pipes reached EOF with no overflow/read error; stderr was empty.
  Raw stdout was 4417 bytes, SHA-256
  `d8c5d73fb209864e310eda5addde0b03a2900fbc2902f5264d6da3d88d24a0dc`.
- Binary bookends matched. IPv6 and denied-port witnesses were quiet, and the
  owned server explicitly settled as `Completed` before cleanup.
- The successful root and outside canary were removed only after complete
  settlement; root absence was independently checked.

No real credentials, paid call, external provider, real-provider authentication
or native tool execution was exercised. The CLI-reported model name is metadata
for a scripted exchange, not proof of that model's service availability/behavior.

## Initialization failure and supported configuration

Two initial protocol attempts timed out. The instrumented retry observed no
provider HTTP; the first attempt lacked request diagnostics and cannot establish
whether a request arrived. The original owned children were killed/reaped at the
fixed deadline; failed fixtures remain retained and are not protocol passes.
A one-second read-only sample
verified the original start/boot identity before and after sampling. It localized
initialization to ICU timezone enumeration in JavaScriptCore. Exact-binary
inspection identified `ucal_openTimeZoneIDEnumeration`, `uenum_count`, and the
negative-count branch to `brk #0` at preferred address `0x105c21f0c`.

A fixed C helper independently observed 463 zones outside the profile, versus
`U_MISSING_RESOURCE_ERROR` and count -1 inside both base and compatibility
profiles. Apple's [ICU directory selection](https://raw.githubusercontent.com/apple-oss-distributions/ICU/main/icu/icu4c/source/common/putil.cpp)
and [resource lookup](https://raw.githubusercontent.com/apple-oss-distributions/ICU/main/icu/icu4c/source/common/udata.cpp)
support explicitly setting `ICU_TIMEZONE_FILES_DIR=""` to skip the external
updated-timezone override and use main-package data. The same helper under the
unchanged profiles then returned 459 zones and `U_ZERO_ERROR`.

Only the new protocol command receives that explicit empty setting. The base
factory, older modes and their artifacts remain unchanged. This selects existing
public data already inside the read boundary; it grants no new filesystem or Mach
access and copies no system data into the fixture. The available bundled main package fingerprinted for this measurement is
`/usr/share/icu/icudt78l.dat` (35,444,880 bytes), SHA-256
`666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549`.
The differing zone counts are preserved; current timezone accuracy and the full
ICU runtime closure are not qualified. Current Apple sources explain the selector;
the actual helper establishes behavior on this host.

The configured CLI subsequently reached the local endpoint with `?beta=true`;
the original exact-target guard rejected it. That failure is also preserved.
The narrowly tested target correction added only this exact SDK query form. Final
success was rerun after all oracle/review fixes; earlier timeouts were not waived.

## Exact bindings and verification

Artifacts live under `/private/tmp/harflow-claude-protocol/`. The final root was
`harflow-os-measurement-8082fe13316306b913c29a13cebb318a`, device 16777230,
inode 602225985. Generated vendor profile bytes and both framed configuration digests were independently
reconstructed and matched the logged values. Diagnostic JSON cannot create a
trusted qualification or launch token. The first unconfigured helper's copied
base-label metadata correction is documented beside its preserved raw result.

| Binding | SHA-256 |
| --- | --- |
| Claude executable | `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d` |
| Vendor base profile | `bfb99059e16622aee50da43a27c4f3d85e4244e690c611a4a6d8556dfaca23ca` |
| Vendor compatibility profile | `59fc0343afc25404070b57bb7f4ab4ce5eec071cc3af64e93cee9dfbbc192828` |
| Declared version configuration | `9aad7eb8da5353fbd7f4427283cf1d0a7f8e1fb6948f46f09a848ad2559411be` |
| Declared protocol configuration | `834eccb30bf24f1a16d94aca6c0f29ca096bc6ef39fb1de921577cbd72b38d40` |
| Final vendor log | `c82b94ef20d1f13e6b788247203e34f9a0894288fae8f9f5893b467741006135` |
| Unconfigured ICU helper result | `314a141c4020d9913ea06c8db9bce29ec75c25be7fd70e1d2f74c349f2ee6a12` |
| Bundled-data ICU helper result | `af87598cb8ade862e406638d486b1c704d2ff5b249e92c66d0e6b94eb1a44fee` |

Final locked workspace: 378 passed, 17 ignored. Focused codec tests: seven passed;
focused host tests: twelve passed, six ignored. The protocol probe, retained-port
ownership regression and eight existing OS safety cases were explicitly run and
passed separately. Formatting, locked check, all-target Clippy with warnings
denied, architecture/legacy-name/whitespace guards and 22 Python tests passed.
Independent plan/source reviews closed within three rounds; the final actual
probe ran after the accepted fixes. No hosted CI or other platform is qualified.

## Remaining gates

This supports only the exact isolated local scripted no-tools/no-fork text protocol.
It does not grant `QualificationState::Passed`, production backend admission,
caller/grant authority, exclusive production-root ownership, complete runtime
identity, arbitrary descendant containment, production resource envelopes,
authenticated live-provider behavior or tool-enabled operation. Other backends and
OS/runtime combinations remain unqualified. No installation, registration,
public capability, client runtime selection, push, release or sibling/tracker
change is included.

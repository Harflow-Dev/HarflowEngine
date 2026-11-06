# Isolated live Claude authentication and egress

Status: owner-authorized on 2026-11-06; implementation and source review completed.
The actual fake-token TLS control failed before the real-credential branch. See
[the experiment receipt](../claude-live-auth-egress.md). No authenticated provider
result or TLS rejection qualification is established. This is a private diagnostic,
not production admission, public capability or aggregate migration acceptance.
HarflowMCP remains the canonical engine and public contract owner.

## Scope and separation

The preceding [gated-exec receipt](../claude-owner-gated-exec.md) qualified an exact
owner-hosted local scripted-provider composition. Its HFCQ1 command remains fixed
to a fake key and nonce-bound local provider. It is not modified for live secrets.
This stage separately tests authentication and egress with a retained direct child.
It does not establish live-provider owner composition, a durable launch handoff,
restart/replay safety, another backend or production containment.

The owner explicitly permits existing credentials and billable calls. No tools,
fork capability, ambient HOME, production admission, push or release is authorized
by this experiment. The existing physical Claude 2.1.318 path/hash remains pinned;
use an optimized local build without changing the Rust pin or dependencies.

## Authentication

Only the trusted controller reads credentials. It invokes fixed `/usr/bin/id -un`
and `/usr/bin/security find-generic-password` for service `Claude Code-credentials`
with the OS-derived account selector. An unqualified service lookup can select an
unrelated MCP record and is not sufficient. Reads have an eight-second shared
observation limit and 64-KiB capture limit, with original waits and physical EOFs.
Errors never include command output or credential contents. Unknown actors remain
under the active original waiter; returning an error does not detach them.

A bounded codec rejects malformed or duplicate JSON keys, invalid access bytes,
missing inference scope and insufficient token lifetime. Only the access token is
passed to the child, via a volatile cleared environment; no refresh token is
forwarded. The opaque value has no Debug, Clone or serialization implementation.
No secret is placed in a preparation, store, plan, log, command argument or digest.
The broker does not log in, refresh, change Keychain access or rewrite the account.

The separate live constructor removes `--bare`: that mode intentionally excludes
OAuth and Keychain authentication. It sets `CLAUDE_CODE_OAUTH_TOKEN`, leaves API
key/base-URL overrides absent, and retains fresh private HOME/config directories,
empty tools/MCP/settings sources, no Chrome/session persistence, and the fixed
nonce-only prompt. Background/nonessential traffic is disabled. Unexpected
startup, managed-policy or authentication behavior refuses without an ambient
configuration, helper-command or credential fallback.

References: [Claude authentication](https://code.claude.com/docs/en/authentication),
[bare mode](https://code.claude.com/docs/en/headless),
[authentication environment variables](https://code.claude.com/docs/en/env-vars).
The exact installed version remains the experimental authority; current online
reference behavior must be checked against actual observations.

## Egress and TLS

The vendor remains under the existing no-fork profile and may connect only to a
fresh, exclusively held loopback port. No direct Internet, DNS, Mach or Keychain
grant is added. Credential-file writes gain an additional denial.

A source-owned trusted proxy outside the vendor sandbox accepts one exact nonce
preflight GET and one `CONNECT api.anthropic.com:443`. It requires the exact Host,
rejects duplicate/authorization/unsupported headers and every other destination,
and permanently revokes forwarding after an invalid request or extra connection.
It freezes one resolved public upstream address, rejecting private, reserved and
IPv4-mapped IPv6 addresses. There is no redirect or connection fallback.

The proxy forwards opaque TLS with finite buffers and 256-KiB limits per direction.
It neither terminates TLS nor receives the inference credential as plaintext.
TLS keys, raw CONNECT headers and tunnel bytes are not logged. No trust override,
extra CA, insecure-TLS setting or TLS-key logging is inherited. A fake-token test
must first observe rejection of a self-signed certificate from an original owned
local TLS helper, with no application plaintext received. Timeout or an incomplete
handshake alone does not establish certificate rejection.

The real client uses the default first-party HTTPS origin. Proxy TLS does not
independently attest billing-account identity. One CONNECT does not prove one
HTTP request: keepalive, HTTP/2 and application retry can reuse a tunnel. The CLI's
configured zero retries, one turn, 128 output tokens and USD 0.01 budget are controls,
not proven provider request-count, replay or invoice limits. Report any CLI cost as
a CLI estimate, not an observed charge. These stronger guarantees remain unqualified.

## Evidence, privacy and settlement

One absolute 130-second observation envelope covers distinct fresh negative and
real instances; creating another root does not reset it. Each instance requires
same-profile five-second safety preflight and a 15-second version/hash phase.
The negative TLS phase has a 15-second cap; credential reading has eight seconds;
the live vendor has 90 seconds, all capped by the shared deadline. Nonpreemptible
OS calls and indefinite safe custody do not have a hard teardown guarantee.

The live invocation requires a nonce witness before exactly one successful terminal
record, no tool/trailing activity, Code(0), both EOFs and lossless capture. Captures
remain in memory; no raw live output is logged. Errors are static categories.
Original identity-checked handles alone own wait/signal authority. No paid retry
follows an uncertain outcome. The proxy/helper must join, their owned sockets close,
and denied listeners stay quiet before cleanup.

After actor settlement, a bounded no-follow scan checks up to 1,024 owned entries
and 32 MiB for the literal access token. Credential-file write denial plus this
scan is a checked diagnostic observation, not a universal nonpersistence or secure
erasure guarantee. If found, qualification fails. Only the original matching
harness-owned files may be removed after identity checks and positive actor
settlement; remaining nonsecret evidence stays private. Unknown actors prohibit
removal. Existing user credential files and Keychain records are never cleanup targets.
After a persistence finding and positive actor settlement only, exact-file privacy
removal may use a fresh five-second observation window. This is post-failure
cleanup, not an extension of the 130-second qualification envelope; that attempt
can never qualify afterward. Like safe actor custody, nonpreemptible cleanup has
no universal completion guarantee.

No result is established until the reviewed offline controls and actual primary-run
experiment agree. A subsequent receipt must retain failures, exact runtime/profile
bindings, authentication mode, observed egress and settlement, and these limits.

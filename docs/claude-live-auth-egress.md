# Live Claude authentication/egress experiment receipt

Status: implemented and independently reviewed; actual qualification stopped at
its fake-token TLS control on 2026-11-06. **No authenticated provider request was
made by this experiment. No real token was supplied to Claude.** Live authentication,
egress and live-owner composition remain unqualified. HarflowMCP remains the
canonical engine and public contract owner; production admission stays refused.

The [reviewed decision](decisions/claude-live-auth-egress.md) defines the exact
private direct-child scope, OAuth handling, proxy, bounds and unqualified claims.
The preceding owner-hosted scripted-provider qualification remains a separate
result. HFCQ1's fixed fake-key/local-provider semantics were not changed.

## Authentication and implementation review

The named API-key/auth environment variables and known config API-key fields were
absent. An account-unqualified Keychain lookup selected MCP OAuth state; selecting
the OS account explicitly found an existing Claude OAuth access token with a
sufficient observed lifetime. Credential contents stayed in short-lived memory;
only availability/expiry booleans were logged. This availability review was not an
authentication success, and was separate from the probe's credential broker.
The probe never reached its broker, inference-scope codec or real-credential phase.

The implementation adds a separate pure live command, opaque bounded credential
codec, account-bound read-only broker, destination-locked CONNECT proxy, credential
write denial and bounded literal-token scan/removal. No new dependency, unsafe
code, vendor/Rust pin, public contract, capability or client selection was added.
The vendor remains no-tools/no-fork and loopback-only; the trusted proxy's external
route is fixed to one resolved public `api.anthropic.com:443` address. Proxy TLS is
opaque, without added CA, TLS-key logging or trust bypass. Original-child custody
and socket/thread joins precede cleanup. No refresh token reaches the child.

Independent review used three rounds, then one extra pass explicitly authorized
by the owner after runtime input defects were found. Changes included active
original wait/EOF custody instead of static abandoned handles, strict proxy poison,
actual upstream-peer binding, true external read canaries, shared phase deadlines,
TLS certificate-alert evidence, settlement before secret removal, and safe metadata
logging. Host lifecycle work was escalated from the test writer to a Sol implementer;
the dedicated Luna test writer retained offline regression coverage.

## Dispatch history

1. Initial invocation omitted the explicit version/hash environment declarations.
   It refused at selection before creating a fixture or launching an actor.
2. Corrected declarations reached the first TLS-helper spawn input guard. The guard
   refused the configured logical Python symlink before `posix_spawn`; no helper,
   Claude or broker was launched. Inspection also found duplicated default
   environment keys. The existing physical-executable and unique-environment guards
   were preserved; the caller now canonicalizes the trusted helper path and adds
   defaults only when absent. Native `/usr/bin/true` regressions exercise the actual
   corrected environment through the same spawn seam. Review round four accepted
   these fixes without expanding grants or changing the vendor pin.
3. The corrected, reviewed optimized invocation ran the negative instance. Controlled
   spawn positive, no-fork/no-spawn, external-read denial, inside read, allowed/denied
   network and protected-write controls passed. Exact version 2.1.318, vendor/ICU
   hashes and metadata bookends passed. The fake-token TLS invocation then failed
   the required clean-refusal predicate. The measurement ended with one failed test,
   zero passed, two filtered out, in **17.12 seconds**.

The final failure was `untrusted TLS negative did not refuse`. That predicate
combines complete bounded observation with a nonzero original exit. The log did
not record the failing capture/exit flags, proxy outcome or TLS-helper verdict
before the assertion. It therefore does **not** distinguish timeout, another exit
condition or the underlying startup/TLS cause; it is not evidence that an untrusted
certificate was accepted. Do not infer certificate rejection from an incomplete
handshake, elapsed time, absence of later traffic or a reconstructed result.

The real branch follows successful completion of the negative control in source;
it was not reached. Neither a live proxy route nor a real-credential vendor was
created. This is not a failed authenticated request, an invoice observation or a
claim of one provider request. No authenticated retry, deadline enlargement,
certificate-trust exception or fork/tool fallback followed the refusal.

Original RAII waiters and proxy/helper joins remain active on unwind; the test
process returned after those cleanup paths. Their individual result values were
not recorded as a separate success receipt. The failed fixture and sibling canary
remain retained. A later read-only bounded scan of its six files (3,187 bytes)
found no literal fake token; this says nothing about real-token persistence.

## Exact observed bindings

| Binding | Observation |
| --- | --- |
| Platform | macOS 26.6.2 / 25G83, arm64 (reobserved after the control failure) |
| Vendor | physical `/Users/nayanda/.local/share/claude/versions/2.1.318`, 319598464 bytes |
| Vendor SHA-256 | `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d` |
| ICU file | `/usr/share/icu/icudt78l.dat`, 35444880 bytes |
| ICU SHA-256 | `666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549` |
| Negative nonce / exclusive port | `63c4c54d418abfb8845dea529feea8e9` / `50426` |
| Negative profile SHA-256 | `51ab433dbbbfb79dc8e567e50271b5920c8990f1cc97b5d604b9380fd348867b` |
| Retained root identity | device 16777230, inode 603767632, mode0700 |
| Retained root | `/private/var/folders/cx/wx59rnx154n9dl6qp7nyl_sw0000gp/T/harflow-os-measurement-63c4c54d418abfb8845dea529feea8e9` |
| Final control log | `/private/tmp/harflow-live-claude/run.log` |
| Final control log SHA-256 | `f93516c9e6e5ec6ad0c0c1c20e4de513f64464199a90be6df0afd6975fc27465` |
| Setup refusal log SHA-256 | `162335ee10dc77799b34d80a68405e90823b66553be07e1099131c040e87e70f` |
| Helper-spawn refusal log SHA-256 | `ef25af9236e23b82b86285b25754670dfbda777675e6eff9985fddebcb3cc1ee` |

The base was `71d482a`, with preserved concurrent ST04 working-tree code/export
present during builds. This is not a clean distribution or reproducibility claim.
No publication, push, release, account mutation or credential installation occurred.

## Verification and next gate

The local locked workspace passed **434 tests, zero failed, 25 ignored**. The
separate opt-in measurement above failed and is not included in that green count.
The 13 new offline controls cover command construction, static credential errors,
duplicate/escaped JSON keys, proxy poison/relay, owned scans/removal and native
spawn environment validation. Both corrected input regressions passed independently
in the optimized profile. Formatting, workspace check, architecture/legacy-name
checks, 22 Python tests, scoped all-target Clippy excluding hf-platform plus platform
library Clippy passed. Full all-target Clippy remains blocked by the untouched
concurrent ST04 `tests/st04_helpers/support/case.rs:209` unbuffered-bytes lint.
No hosted CI, other platform or other backend is qualified.

The next useful experiment is **fake-token-only startup/TLS diagnosis** with capture
status, proxy and TLS-helper metadata collected before any failure assertion.
Give it an explicit entry point that cannot call the credential broker or real
branch, preserve the existing strict TLS oracle, and keep all current grants,
deadlines and pins. No further authenticated attempt is justified until that
control produces actual rejection/no-application-data evidence. Existing owner
authorization for credentials and billing remains valid; it was not exercised here.

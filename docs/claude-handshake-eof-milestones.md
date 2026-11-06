# Repeated Claude handshake EOF: native milestone diagnosis

Status: implemented; actual local milestone controls passed. No new installed-Claude
run has occurred in this stage. The prior [four-total result](claude-replacement-wave-negative.md)
remains failed; no retrospective certificate classification is possible.

## Question and boundaries

Two independent source diagnoses found client closure during the initial TLS
exchange best supported, while certificate verification versus exact-runtime
compatibility remained unresolved. The existing `Handshake` phase only says that
`do_handshake()` did not return; it does not identify the last native protocol event.
CONNECT corruption or fifth-socket poison causing those already-retired EOFs is
poorly supported by the source and retained settlement witnesses.

This discriminator keeps four total/two active connections, the original overall,
preflight/version/negative/helper deadlines, shared byte/pending/header/scratch
budgets, exact vendor/ICU pins and isolated no-tools/no-fork profile. It adds no
trust, cipher, ALPN, TLS-version, verification or upstream change. No credentials,
provider, production admission, push or release is permitted.

## Closed observation

The pinned CLT Python 3.9.6 / LibreSSL 2.8.3 exposes `_msg_callback` and
`sni_callback`. The new observer registers each actual accepted socket with its
ordinal and numeric peer before progress, up to four records. Native callbacks
set a constant-size 12-bit allowlisted milestone mask and a fixed SNI category.
Callback payload and version arguments are never inspected, copied, hashed,
parsed, stringified, stored or logged. The library callback covers TLS protocol
messages, not application data. SNI is compared only against the existing fixed
authority; no raw name is retained or emitted and no context or alert is selected.

Known incidental record-header and ticket callbacks are explicitly classified.
Unknown native descriptors, unregistered identities, overflow or callback errors
produce closed failure metadata. Callbacks return None and handle runtime errors
so observation does not deliberately change handshake outcome. Unsupported
instrumentation cannot qualify. A zero-event admission still retains its identity.

`TLS_MILESTONE_V1` rows follow the unchanged READY/SESSION_V2/DONE transcript and
carry only ordinal, literal loopback address, peer port, mask and SNI enum. A strict
suffix codec validates complete one-to-one ordinal **and** port coverage, known
bits/enums, ordering and the original 4096-byte capture bound. The old codecs and
modes retain their prior behavior. The distinct ignored diagnostic entry requires
`HARFLOW_RUN_HANDSHAKE_EOF_DIAGNOSTIC=1`; it selects only the existing fake-only
four-total proxy and the instrumented local helper. The strict certificate gate
is unchanged, with valid additional metadata also required.

A write flag means the native library reported that protocol event; it does not
prove wire delivery, client receipt or acceptance. An unset bit means **not
observed**, not proof of wire absence. SNI match is not certificate validation.
Proxy forwarding/EOF, helper exception classification, original process settlement
and native milestones remain separate witnesses. None turns EOF into rejection.

## Local controls and result

The optimized local target passed three tests in 0.43 seconds. Actual closed rows
passed the Rust suffix codec against all four independently recorded peer ports:

| Local peer | Native milestones observed | Independent outcome |
| --- | --- | --- |
| Trusted completion | ClientHello, full initial server flight, client key exchange, both CCS/Finished; SNI matched | application data, no certificate rejection |
| Default certificate verifier | ClientHello, full initial server flight, inbound alert; no client key exchange or Finished observed; SNI matched | explicit certificate-alert rejection |
| Generated ClientHello then write EOF | ClientHello, full initial server flight; no client key exchange, Finished or alert observed; SNI matched | handshake transport EOF; certificate verification never performed by this peer |
| Raw EOF | no milestone events observed, SNI unseen; admission identity retained | handshake transport EOF, no certificate rejection |

The initial server flight here comprises native write reports for ServerHello,
Certificate, ServerKeyExchange and ServerHelloDone. The deliberate early-close
peer drains bounded bytes without consuming them through its client TLS object;
it does not verify or reject the certificate. This prevents the matching EOF
pattern from becoming invented certificate proof.

The controls additionally cover a registered valid callback with poisoned payload
and version arguments, unknown/unregistered descriptors and overflow, zero-event
registration, strict suffix identity/enum/mask validation, and no sensitive error
echo. Cleared environments, fixed CLT interpreter, bounded original process
capture/exit/physical EOF and nested helper wait/reader custody are retained.
Unexpected test-driver failures emit fixed markers.

Final original-mode regression/static checks and independent review precede one
newly discriminating fake-only Claude measurement under the unchanged bounds.
No unchanged rerun or larger allowance is selected.

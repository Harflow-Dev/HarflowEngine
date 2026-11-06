# Proposed two-tunnel fake-token TLS control

Status: reviewed proposal pending explicit owner decision. Not implemented,
selected or authorized for execution. The existing one-CONNECT mode remains
unchanged. No new public contract, capability or production grant is proposed.
HarflowMCP remains the canonical public contract owner.

## Problem and discriminating question

At `24fafd8`, the exact pinned Claude fixture opens an extra IPv4 socket while the
first CONNECT tunnel is active. The proxy observes it 27 microseconds after tunnel
entry, before reading or forwarding any tunnel bytes. Its reviewed one-CONNECT
rule poisons forwarding immediately. The strict TLS negative then remains
unqualified: a handshake interrupted by the controller cannot demonstrate the
vendor's rejection of the self-signed certificate.

The [current decision](claude-live-auth-egress.md) explicitly restricts this private
experiment to one nonce preflight and one CONNECT. That is a binding experimental
admission policy, not evidence that all production clients must use one socket.
It does not prove one HTTP request, replay safety or a billing cap. Unknown socket
purpose neither establishes maliciousness nor authorizes relaxing the policy.

Question: if at most two same-destination tunnels can progress concurrently,
within the original aggregate bounds, does the pinned vendor produce an explicit,
independently witnessed certificate refusal? This tests whether the one-tunnel
restriction masks the TLS control. It does **not** identify an HTTP endpoint,
startup function, retry intent, account identity or production compatibility.
Another timing-only run in the unchanged one-tunnel mode would not resolve it.

## Exact requested delta

Add a separate, explicit fake-only diagnostic mode. Keep the current one-tunnel
mode and all live/owner routes unchanged. The new entry must be unable to call the
credential broker, select a real upstream or continue into a live phase.

| Boundary | Proposed measurement |
| --- | --- |
| Vendor | Existing physical Claude 2.1.318 path/hash and ICU/Rust pins |
| Inputs | Existing fresh private config, literal fake token, prompt, no tools/fork |
| OS grants | Identical profile; exclusively owned numeric loopback proxy port only |
| Controller preflight | Exactly one nonce GET, required before tunnel admission |
| Vendor connection budget | At most two accepted post-preflight sockets for the entire instance, including malformed/refused sockets; no recycling |
| Advertised route | Only literal `CONNECT api.anthropic.com:443 HTTP/1.1` with the existing exact Host and header allowlist |
| Physical destination | Only the single fresh owned `127.0.0.1:<helper-port>` TLS helper, frozen before vendor launch; no DNS or Internet resolution |
| Concurrent forwarding | At most two validated tunnels; each has a retained connection identity and independent capture-free relay state |
| Bytes | Existing 256 KiB per direction shared across both tunnels, including bytes read but not yet forwarded; no per-tunnel multiplication |
| Buffers | Existing 16 KiB of pending data per direction shared across active tunnels, not doubled; bounded fair multiplexing and one fixed read scratch buffer |
| Observation | Existing 130-second absolute envelope, five-second safety preflight, 15-second version phase and 15-second negative phase; no per-connection reset |
| Helper | One retained trusted helper process; at most two TLS sessions, concurrent within the existing 25-second absolute readiness lifetime |

The material change is admission and validation of a second socket that currently
is closed unread. Its CONNECT envelope must be parsed in volatile bounded memory
using the existing route/Host/header rules. No tunnel payload or decrypted HTTP
request is parsed. No CONNECT headers, destinations from invalid inputs, request
contents, raw TLS records, secrets or raw errors are logged. The fixed allowed
route is configuration, not an attribution of the observed extra socket.

A third socket, any IPv6 attempt, duplicate preflight, malformed/unsupported
CONNECT, forbidden header, destination mismatch or exceeded shared bound
permanently poisons the whole instance. Stop all forwarding, retain and settle all
original actors, and mark the attempt failed. No queue, retry, replacement or
upstream fallback may turn refusal into a new allowance. At each relay step,
apply poison/deadline checks before additional forwarding. New header reads remain
bounded by the existing 8 KiB per accepted socket and the shared phase deadline.

## Required offline evidence before any vendor run

Implement and independently review this mode only after approval. Keep source
changes separate from ST04. Tests must exercise actual local socket behavior,
without sleeps as synchronization or extra connections slipping through cleanup:

- Two admitted IPv4 tunnels relay distinct opaque fixture bytes concurrently to
  the same frozen helper address, with per-connection identities and full closure.
- Third connection, IPv6, malformed/duplicate/authorization header, wrong Host,
  other destination and duplicate preflight each poison both tunnels. Independent
  upstream receivers and proxy write counters prove no additional forwarding after
  the poison transition; receivers may still drain bytes written before it.
- Aggregate bytes/buffers are shared; alternating use of two sockets cannot double
  the budget. Slot closure does not restore the total admission budget.
- A partial/idle second header cannot block the first tunnel or extend the shared
  deadline. Deadline, I/O failure and controller stop retain first-cause metadata.
- The original one-tunnel tests continue to refuse the second socket. The new
  command path has no broker, live proxy, real credentials or provider call.
- Fixed loopback denied listeners independently stay quiet for forbidden vendor
  routes, and the unchanged no-fork/read/write controls still pass.

The TLS helper needs its own independent tests: valid controller client can reach
application-data detection, a client that rejects its certificate produces an
explicit certificate-alert verdict, and EOF/timeout/controller stop never becomes
certificate rejection. Per-session results must correlate to proxy connection IDs
using trusted loopback peer tuples; the collector must reject absent/duplicate or
unmatched session identities. Labels and boolean outcomes alone are insufficient
without this binding.

## Run, verdict and settlement

After green offline tests and final independent review, run at most one exact
ignored fake-only vendor measurement. No automatic rerun, live follow-up or token
inventory follows it. Keep the original strict CLI nonzero exit, static certificate
error, complete lossless capture and both physical EOF requirements. The helper
must independently observe explicit certificate rejection on every admitted
session, zero application data and complete session/identity coverage. If the
second socket remains idle or never becomes a valid tunnel, the TLS control stays
unqualified; a speculative socket is not a reason to weaken this criterion.

Record only fixed stage/reason enums, connection ordinal/family, matched owned peer
identity, timing relative to defined controller events, opaque byte counts,
EOF/half-close/closure flags, certificate-alert booleans and application-data
presence. Do not read or persist vendor output for diagnosis beyond the existing
bounded in-memory static classifier. This mode does not enable vendor debug logs,
TLS key logs, packet capture or content-bearing traces.

Independently witness original vendor exit and both EOFs; proxy worker joins and
all client/upstream socket closures; helper original exit, both EOFs and all session
settlements; denied-listener silence; and the existing bounded literal fake-token
scan after settlement. Keep failed roots/canaries and distinguish observation at
deadline from later cleanup. Unknown actors keep active original custody; they
prohibit cleanup and never qualify through forced exit. Existing safe cleanup
exceptions do not extend the qualification envelope.

Any application data, excess connection, mismatched identity, silent/incomplete
handshake, capture loss, unknown settlement or ambiguous verdict fails closed.
Even a fully passing result qualifies only this fake-only two-tunnel negative
control. It does not admit real credentials, paid requests, owner integration,
R04/R05 acceptance, production execution or a new public route.

## Threat-boundary tradeoff and recommendation

The proposed gain is to let TLS progress past an experimentally chosen socket cap
while preserving a finite aggregate resource envelope and the same loopback-only
vendor sandbox. The cost is a second admitted/parsed connection, concurrent relay
and helper state, additional parser exposure, and a more complex all-connections
settlement proof. Destination locking, shared budgets, independent identity binding
and global poison must be demonstrated rather than assumed from the original
single-tunnel implementation. The proposal broadens experiment admission; it is
not a harmless logging adjustment or a production security conclusion.

Recommend approving only this distinct fake-only mode if its narrower question is
useful. If identifying the exact startup caller is the required outcome, this
proposal is insufficient: it deliberately does not inspect the second request's
contents. Declining it leaves the current strict TLS gate unqualified and all
existing admission constraints enforced. No push, release, install, registration
or production runtime selection is included in either decision.

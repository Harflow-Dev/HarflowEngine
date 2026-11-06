# Proposed pinned-Bun short-record control

Date: 2026-11-06. Status: proposal; no implementation or vendor execution is
selected by this document. The previous one-measurement authorization is spent.
A new explicit approval is required for one additional pinned `--version` control.
There is no authorization for another TLS capture, broader hook, trust change,
credential/provider use, production admission, push or release.

## Question and discriminator

The [hook-phase measurement](../claude-client-hook-phases.md) emitted READY only.
Four TLS sessions failed, but no successful short-record emission was observed
inside the pinned Bun process. Alternative native routing, later hook replacement
and recorder unavailability remain distinct from the unknown TLS close reason.
Node controls establish component behavior, not this exact embedded runtime.

One synchronous synthetic function under the existing hook exercises short-row
emission inside the exact pinned process without using global fetch or TLS.
A successful control would establish this bootstrap recorder/hook path only.
It cannot establish asynchronous promise coverage, later recorder health, native
fetch routing, certificate rejection or a production-compatible backend.

## Fixed control source

The proposed protected entry imports the unchanged current hook/writer/classifier.
It neither reads nor replaces `globalThis.fetch`. Its only error is an owned
synthetic sentinel, whose identity and single invocation must be observed before
registering final completion. No error text, content or CLI output is emitted.

```javascript
'use strict';
(() => {
  try {
    const { createWriter } = require('./client_reason_writer.cjs');
    const { install } = require('./client_reason_hook.cjs');
    const writer = createWriter();
    if (!writer.write(['CR2'])) return;
    const sentinel = new Error();
    Object.defineProperty(sentinel, 'code', { value: 'ECONNRESET' });
    let calls = 0;
    const observer = install(function () {
      calls++;
      throw sentinel;
    }, writer);
    let identical = false;
    try { observer.proxy(); }
    catch (error) { identical = error === sentinel; }
    if (!identical || calls !== 1) return;
    process.once('exit', () => {
      try { observer.finish(); } catch (_) {}
    });
  } catch (_) {}
})();
```

The exact control-specific metadata sequence is five tab/LF-delimited rows:

```text
CR2 <nonce> <original PID>
CI2 1
CS2 1
CE2 <nonce> <original PID> 1 reset missing
CX2 <nonce> <original PID> 1 1 0 1
```

Maximum size is 180 bytes at a 32-character nonce and ten-digit PID, within the
unchanged 512-byte bound. Generic parser success is insufficient: extra/missing
rows, another category, identity mismatch or unhealthy completion must refuse.
Synthetic categories stay exclusively in the side channel and can never be fed
to the original CLI certificate-refusal classifier.

## Proposed gated lifecycle

A separate ignored entry gated by `HARFLOW_RUN_CLIENT_SHORT_RECORD_CONTROL=1`
would execute only the exact pinned Claude 2.1.318 `--version`, SHA-256
`677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`.
The current fixed `BUN_OPTIONS=--preload=<protected entry>` seam and OS policy
remain unchanged. Copy the fixed entry plus unchanged hook/writer/classifier into
a new protected fixture root, with aggregate source at most 16 KiB and source
identity/hash bookends. Never append to a retained measurement root.

Reuse the inert preload-support lifecycle: 130-second overall, five-second
preflight and one shared 15-second pins/version window. Retain one identity-bound
512-byte metadata file; unused helper allowance stays unused. No TLS helper is
started. Keep the four-total/two-active proxy configuration, but require zero
post-preflight accepts/CONNECTs and a quiet owned target listener. Denied IPv4/
IPv6 listener, canary, vendor/ICU/source and bounded scan witnesses remain required.
No new descriptors beyond existing reporter custody, timers or runtime flags.

Settle the original child and proxy before independently collecting all witnesses
and asserting success. Require exact original 22-byte version output, empty stderr,
Code(0), no expired observation, positive physical EOFs and exact retained PID
binding. Clean the successful root only after positive custody; retain failures.
The existing TLS negative gate is not exercised or satisfied by this control.

## Required offline vectors before any approved vendor execution

| Input/control | Required result |
| --- | --- |
| Fixed stub throws the exact sentinel once | Exact five rows; original version streams unchanged |
| Stub returns, throws a different object or runs twice | No qualifying final completion |
| Writer fails at first short CI2 row after READY | Original stub call/error preserved; unavailable control |
| READY write fails | Return before invoking the stub; unavailable control |
| Wrong nonce/PID/category, missing/extra/torn row or unhealthy end | Static refusal, no input echo |
| Path replacement/symlink/hardlink/mode/size violation | Existing retained-file refusal remains strict |
| Cleared-environment retained Node child | Five-second/512-byte contract fixture only, never Bun evidence |

## Interpretation and stop

Exact sequence plus all witnesses proves synchronous Proxy entry, short-row
emission, sentinel classification and completion at bootstrap in this pinned
Bun/profile. READY-only remains unavailable without a selected cause. CI2 without
later rows proves only that one short row was emitted; phases without CX2 remain
incomplete. Unexpected CLI bytes, connection or custody failure refuses.

There is no automatic follow-on TLS run or widening after either outcome. Review
and implementation checks precede a separately authorized single control run.
The offline R06 storage/state work is independent and does not require this result.

An independent proposal review approved the logic, 180-byte maximum and authority
boundary. No implementation or control execution has occurred. The required
offline vectors and any concrete implementation need validation before a separately
authorized vendor control can run.

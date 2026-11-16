# E2-B4 recovery binding and precommit journal foundation

Delivered locally on 2026-11-16. This stage supplies internal binding and journal
primitives; it does not complete B4 recovery or expose a new ctl capability.

Application binding distinguishes fresh admission from resumption with a held
session and explicit lock identity. Maintenance sessions recheck the optional
legacy exclusion lock as well as their existing root and Rust lock identities:
appearance refuses as busy, and disappearance or replacement invalidates the
held session.

The journal codec admits only Planned, Prepared and BackedUp precommit phases.
It checks versions, field relationships, identity and digest shapes, bounded
inventories, duplicate fields and unsupported extensions. Encoding uses sorted
compact ASCII JSON with a final newline and standard base64, bounded to 16 MiB.
Decoding checks input size before parsing and walks inventories iteratively with
100,000-entry and 128-depth limits. Unsupported completed phases and staging
extensions are refused. Hashing actual inventory and previous bytes belongs to
the later recovery flow; this codec validates their representation and pairing.

No pending record or journal is published, no archive or move occurs, and B5
staging remains absent. The canonical Python engine and current client pins are
unchanged.

## Evidence

- Final full Rust workspace: 2,109 passed, zero failed, 63 ignored; 304 summaries.
- Full Python suite: 937 tests, OK with two skips and three existing expected failures.
- Focused families include seven application foundation tests, nine journal codec
  tests and eleven maintenance identity tests.
- An independently generated Python JSON oracle was regenerated and compared
  byte for byte with the committed fixture.
- All-target clippy with warnings denied, formatting, architecture and legacy-name
  checks passed. Independent review and follow-up review closed without findings.

Kernel and FIFO fixtures required execution outside the sandbox. Tests used
temporary owned roots. No live engine state, provider calls, desktop interaction,
push, release or migration acceptance is claimed by this receipt.

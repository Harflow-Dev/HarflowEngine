# E2-B7 — Recovery read qualification

## Scope

Approved E2-B revision 4 §3.9/B7, implemented from baseline `d89d612`.
`RecoveryReadSession` now runs the existing linear `qualify::precheck` after
the recovery gate, shared lease and I/O gate, before creating a private copy.
The returned marker identity qualifies the copy, and its captured watermark is
compared against the copied SQLite commit sequence before ownership, capture or
event reads. Copying always requires the source WAL.

`Fenced`, `CommitLost`, `WalMissing` and `RecoveryInterrupted` explicitly map to
`RecoveryError::Unavailable`. Missing roots and operations retain their existing
no-record behavior when the source qualifies. No source SQLite connection,
healing, fence/acceptance/watermark mutation, recovery intent, adoption or new
authority is introduced. Schema, dependencies and public wire contracts remain
unchanged. B8 and B9 are outside this slice.

## Invariants and limits

The original shared claim and exclusive I/O gate cover precheck, the private
main+WAL copy, copied sequence comparison and evidence reads. Existing 64 MiB
combined copy cap, 64 KiB buffer, deadline checks, private scratch location,
binding/marker rechecks and scratch cleanup remain in force.

These are cooperating-writer and bounded-admission guarantees. Filesystem,
kernel and SQLite calls retain their existing synchronous latency limits;
hostile writers and the documented path-based scratch cleanup race remain
unqualified. An unavailable observation is never owner-death or release proof.

## Validation

Final primary evidence is retained outside this repository in workspace
`.claude-work/codex-2026-11-16/b7-final-gate/` and `b7-final-run.log`.

- Focused recovery observations: 17/17 passed, including the deterministic I/O
  lease barrier, copied sequence qualification, fence and recovery-gate refusals,
  missing/malformed watermark, missing WAL and invalid sidecar types. Nonregular
  and symlink WALs retain the earlier I/O binding's exact `Invalid` refusal;
  a genuinely missing WAL reaches `WalMissing` and maps to `Unavailable`.
- Complete workspace: 2195 passed, zero failed, 65 existing ignores, 312 result
  summaries. No ignore was added. Complete Python suite: 951 tests, successful
  with 2 skips and 3 expected failures.
- Formatting, architecture inventory, legacy-name guard, strict workspace/all-target
  Clippy and release builds passed. Retained release binaries have SHA-256 hashes.
- Strict seed replay: 59 worlds, 926 verified allowlisted additions, zero unverified
  additions and zero unexplained physical size differences.
- Pinned conformance profiles: full 56/0, deltas 55/1, baseline 50/6. Failure
  identities match the retained B3 baseline exactly; historical gaps are not waived.
- Independent source review closed after three rounds. Test-only fixture,
  ID-formatting and sidecar-refusal expectations were corrected before the final
  passing execution. No production changes followed the second source review.

All runs used isolated fixtures and a serialized execution lane. No real user
state, provider, publication or client selection was exercised. This receipt does
not accept full E2-B, R14, native integration or the remaining fault/real-version
qualification gates. B8 and B10 remain separate; B9 remains owner-deferred.

## Reported-bug carry-over

The workspace ledger's MCP #117, #112 and #98 obligations are not addressed by
this bounded observational-copy slice: it neither classifies public live-task
continuity, adopts an owner nor settles launch receipts. Those R02/R04/R12
obligations remain attached to their gates; this receipt does not waive them.

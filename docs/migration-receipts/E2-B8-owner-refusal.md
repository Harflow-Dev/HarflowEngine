# E2-B8 definite owner refusal

Local implementation and verification checkpoint, 2026-11-17. This does not qualify public execution or migration acceptance.

Exact storage `Fenced` and `CommitLost` errors retain their type through owner
execution and produce a private `Refused` acknowledgement with a typed cause.
All other execution failures remain `Unknown`. HFOR framing is revision 2 and
rejects revision 1; public ctl schemas, capabilities and storage layout are unchanged.

The coordinator validates the sent command identity, deadline, allowed commit
command, refusal cause and unchanged committed snapshot/capture/raw facts. It
clears pending work without advancing sequence or applying projected evidence and
permanently refuses further commands. Invalid refusal frames poison as before.

The owner distinguishes known synced but unindexed Append bytes from uncertain
raw tails, abandons remaining accepted data without publishing storage loss, and
keeps the reader draining. It retains the actual storage writer and root leases
while the original child and reader settle. Only then is writer input closed with
EOF; no Close command, retry or terminal publication follows. The writer waits
for EOF without storage I/O or deadline-based release. Reservations persist and
an earlier terminal publication stands. Retention can wait indefinitely.

Primary verification used the serial two-job lane and two Rust test threads.
Evidence is retained outside the repository in workspace
`.claude-work/codex-2026-11-17/b8-final-gate/`:

- Formatting, architecture and legacy guards, strict workspace/all-target Clippy,
  and release ctl/seed builds passed.
- Workspace tests passed 2,205 tests, with zero failures and 65 existing ignores.
  These include both isolated storage-refusal integration cases and actual-owner
  lifetime/drainage regressions after the final test-fixture fixes.
- Python tests passed 951 tests, with two skips and three expected failures.
- Strict replay compared all 59 worlds: 926 allowlisted additions verified,
  zero unverified additions and zero unexplained physical-size differences.
- Selected conformance profile passed 56/56. The older delta and baseline profiles
  retained exactly their pinned known failure identities (55/1 and 50/6).
- Independent source review was clear after a second pass over the narrow
  test-fixture and documentation corrections. Final formatting and diff checks passed.

The qualified local release binary SHA-256 values are:

- `harflow-ctl`: `66b24818cdd7bd0f74d35bfbe751f49074457b845d0f3f6d1d0d4b4f8a55edb0`
- `hf-fixture-seed`: `010a3257696473ef4315bf84b2860a412a8b3c559599ee2261fde38f7309c8b4`

No push, release, client pin, dependency or public contract change was performed.
B9 remains deferred; later execution and migration gates remain separate.

If transmission of the refusal acknowledgement fails, the owner has no validated
definite outcome and follows the existing Unknown path. That path may close the
writer and its Store early; it does not provide the stronger Store lifetime of a
received and validated refusal. The original owner's independent `RootHold` and
reservations remain retained until the original child is positively reaped and
the reader is joined and reconciled. No retry or terminal publication follows.
No additional settlement signal is introduced for failed acknowledgement transport.

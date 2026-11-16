# E2-B4 internal archive recovery through pending BackedUp

Local implementation checkpoint, 2026-11-16. This delivers the internal archive
flow through BackedUp while retaining the pending gate. It does not initialize a
new store, publish a committed/completed receipt, expose state-recover, change
client pins or accept a migration gate. B5/B6 and remaining E2-B work stay open.

## Scope and authority

Governing behavior: E2-B plan revision 4, pinned HarflowMCP a891388, existing B1–B3
and binding/journal foundation `0bdf997`. Fresh admission rechecks eligibility,
selected root/operation/engine, actual held lock, normalized proposal inventory
hash, exact prior-journal bytes and size before pending. Layout 2 refusal precedes
other request checks. Current-version completed previous journals are qualified
and preserved as opaque exact bytes; the precommit codec still refuses completed
or B5 extensions.

The original maintenance session retains its exclusions across all effects.
Fresh evidence follows approved Q1(b′); unreadable evidence remains ambiguous,
never synthesized clear. Journaled resumes recheck recorded process identities
without reopening SQLite. New pending records with no established journal retain
the approved fresh-proposal retry rule.

Pending publication precedes the journal, backup preparation and moves. Descriptor
operations reject symlinked ancestors and substituted root/backup/data identities.
Original entries move marker first, then sorted inventory, with volatile SHM last.
Every original entry is checked in exactly one location; conflicts and unknown
entries refuse. Each move syncs both directories. Retry repeats required file and
directory durability barriers. Partial unqualified temporaries are preserved and
refused, never deleted as though ownership were proven. BackedUp resumes skip moves.

## Approved dependency change

Owner explicitly approved exact macOS-only hf-platform `rustix 1.1.5`, default
features disabled and fs/std only, with its matching exact policy entry. Cargo.lock
adds only `rustix 1.1.5` and `linux-raw-sys 0.12.1`. MacExclusiveRename uses pinned directory
descriptors and `NOREPLACE`, validates basenames and has no overwrite fallback. Workspace
`unsafe_code=forbid` and all other dependency restrictions remain unchanged.

## Evidence

- Full Rust workspace: 2,149 passed, zero failed, 65 ignored; 308 summaries.
  The two explicit size checks below passed separately; 63 other ignored tests
  are unchanged from the preceding checkpoint.
- Full Python suite: 937 tests, OK with two skips and three existing expected failures.
- Workspace all-target clippy with warnings denied, formatting, architecture,
  legacy-name and diff checks passed. Both release binaries built successfully.
- Pinned E1 release-binary profiles: current 56 passed/zero failed; intermediate
  55 passed/one known failure; baseline 50 passed/six known failures. Exact failing
  case sets match B3, not merely their counts. The existing unverified
  recovery-activity.capture_loss exclusion remains; this is not R14 acceptance.
- Seed parity: SAME under the existing verified allowlist; 926 additions verified,
  zero unverified and zero unexplained physical-size differences.

Durable workspace logs and immutable binary hashes are in
`.claude-work/codex-2026-11-16/b4-recovery-gate/`.

Real primitive tests cover file/directory/symlink moves, collision preservation and
fault retry barriers. Actual-session composition covers full fresh archive,
byte/inode and stable-lock preservation, marker ordering, 64 injected effect
boundaries (58 successful retries, 6 conservative partial-temporary refusals), 17
SIGABRT restart points, replacement/extra/double-location conflicts and recorded
live/unknown identities with a poison evidence port on journaled resumes.

The integration target serializes its actual-session fixtures because the existing
process-wide lease coordinator uses `try_lock` and may return `Busy` for disjoint
concurrent fixture acquisitions. The 5 s fixture deadline and production behavior
are unchanged; this is not a concurrency qualification claim.

Explicit size checks passed: exactly 100,000 inventoried entries, 5,631,282-byte journal,
8.714 s measured proposal/session/archive; sparse 2,147,483,648-byte logical payload,
0 allocated payload bytes, 28.125 ms measured proposal/session/archive. The latter
checks inode, size and known-zero samples; it is not a physical 2 GiB database or
throughput measurement. Both test fixtures cleaned up after execution.

Core flow and approved rename delta received separate independent reviews. Core
review closed after 3 rounds, dependency/real-adapter review after 2, with valid
findings fixed before final checks. Dedicated unit writer Luna medium and system
integration worker Sol low wrote tests; primary verified independently.

## Limits

Inspection may offer roots whose maintenance evidence is unreadable; recovery
still refuses them. Strict journal provenance may refuse an advisory offer with
foreign/missing top-level engine identity. These do not grant archival authority.
No power-loss/media qualification or general hostile concurrent-writer guarantee
follows from injected faults and process aborts. No B5 staged initialization,
R8/R9 completion, public ctl/capability, signing, deployment, push, live-state use,
provider call or desktop interaction occurred.

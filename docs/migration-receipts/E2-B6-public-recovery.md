# E2-B6 public recovery composition

Local implementation checkpoint, 2026-11-16. Governing design is E2-B revision 4
and pinned HarflowMCP a891388; B5 completion remains the underlying protocol.
The observed verification and remaining limits are recorded below. No engine is
published, no client pin is changed, and full E2-B acceptance remains open.

On macOS arm64, ctl accepts state-recover with required --operation-id and
--confirmation-token plus --json, and advertises state-recovery.v1 immediately
after version-info.v1. Other targets retain invalid-choice refusal and do not
advertise the capability. Success uses the canonical completed result fields;
operational failures use exit 1, stderr and optional JSON, while parser failures
retain exit 2. No read-only caller class is promoted to mutation authority.

The bounded marker-only layout-2 refusal precedes token parsing. Canonical
8192-byte token decoding binds the selected root, current engine and operation.
The safe-root identity and historical receipt checks run without a lease before
MaintenanceSession acquisition. Historical success opens no current store and
requires no maintenance exclusion. Fresh and resumable operations retain the
original session through archive, initialization, commit and receipt publication.
Matching committed operations preserve B5's minimal reconciliation path, bypassing
current inventory, owner evidence and SQLite.

MaintenanceSession exposes only proposal-bound acquisition and a session-bound
opaque files-only initializer. The host storage guard adds exactly that session
type to its approved inventory. Qualified application failures retain progress
from admitted journal facts or a matching committed record; the transport never
derives operation or backup evidence from raw arguments or a later state scan.
The internal archive_only helper remains available with its original error API.

B7, B8, B9 and B10 remain separate work. Build/test execution was serialized
with the native desktop checks; no live user state or provider was used.

Review correction: before maintenance infrastructure exists, bounded descriptor-relative
checks reject a qualified Python marker or any child in the three known Python-state
directories. They bind the proposal root identity and follow no links. Historical
receipt lookup still precedes any current-journal read. Only when no old receipt
answers does a bounded selected-journal check refuse positively qualified Python
producer evidence: matching top/proposal engine versions in the unadorned numeric
release form used by the pinned Python engine. Unknown producer strings and other
Rust versions are not classified as Python and retain conservative protocol refusals.
The refusal remains unsafe_root with recovery_supported false and directs callers
to the Python engine or a fresh root. No current SQLite or inventory scan is added.

Known-directory observation uses its own no-follow opener: permission/I/O failures
report infrastructure_unavailable because contents remain unknown, without
inventing Python-state or backup-conflict evidence. Disappearance is stale; a
link/non-directory substituted during open is unsafe. Successful opens still
compare the descriptor and rooted entry identities.

## Local verification

Evidence is retained outside this repository in workspace
`.claude-work/codex-2026-11-16/b6-final-gate/` and `b6-gate/`.

- Formatting, architecture inventory, legacy-name guard, strict workspace/all-target
  Clippy and the release build passed.
- Exact release binaries are retained with SHA-256 hashes. Strict seed comparison
  passed: 926 verified allowlisted additions, zero unverified additions and zero
  unexplained physical size differences across 59 worlds.
- The focused public recovery system target passed all 3 tests (`primary-focused-r7.log`),
  including archived original identity/bytes, current-owner exclusion, unfinished
  receipt exclusion and durable historical success while the current store is live.
- The complete workspace attempt passed 2186 tests with 65 existing ignores and
  found one stale skeleton CLI expectation that still classified public recovery
  as unimplemented. Its test-only correction and affected-target rerun are recorded
  separately; the initial failed attempt is retained.

The affected Rust skeleton target subsequently passed all 10 tests. The complete
workspace was not repeated after that test-only correction; the original attempt
and final affected-target run together cover 2187 passing definitions and 65
existing ignores.

The complete Python attempt ran 951 tests with one new-test failure, 2 skips and
3 expected failures. The failure assumed a recovered bare Store contained task
catalog coverage. `docs/task-reads.md` explicitly requires unseeded datasets to
return unavailable, and the release diagnostic confirmed compatible inspection
with the documented `state_incompatible` / `infrastructure_unavailable` task-read
refusal. The corrected recovery file passed all 13 tests against the exact retained
release artifact (`python-recovery-final.log`); the full Python suite was not
repeated after this test-only correction. Healthy store initialization does not
claim task-catalog coverage or successful task reads.

Pinned conformance passed its expected profiles: full 56/0, deltas 55/1 and baseline
50/6, with exact unchanged failure identities. This checkpoint does not establish
client integration, full E2-B acceptance, publication or replacement of the
canonical Python engine.

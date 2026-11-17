# E2 first-run source checkpoint

Local source checkpoint, 2026-11-17, based on Engine `9cc845c`. This implements
the experimental macOS arm64 run/status/completion subset. It does not establish
a successful actual task, lifecycle qualification, production adoption or client
integration. HarflowMCP remains the canonical contract owner. Its candidate
addendum is pinned at `4cb9d3270bbe54ee9fcafd287617cb6f4e1c9bd0`, with candidate
manifest SHA-256
`d893c94a7f3406ed9565193104f46359b43b35a63a9e989e63ea9b2a37400a90`.
No supported capability, schema, dependency or client engine pin is promoted by
this checkpoint. README's experimental and unpublished status remains applicable.

## Implemented source boundary

The public run parser admits the narrow Claude/unrestricted request before
effects. The host composes the existing caller gate, explicit selected binary
observations, fresh-root initialization, durable intent and inherited same-binary
owner/writer channels. The original owner retains actual child and capture
handles; the original writer retains storage custody. T2 atomically publishes the
observed spawn and task projection. Raw-source admission and T3 capture/event
publication precede normalized observations; display text supplies no execution
authority. Uncertain construction or publication retains custody and refuses
automatic retry, replacement or release.

T5 composes retained-child exit, actual EOF, reader join/FIFO, input closure,
source binding and fenced terminal publication. Successful root `result.result`
text can populate the completed status summary only when its result witness
matches committed evidence and the entire serialized canonical projection fits
16 KiB. Oversized text is omitted as null without clipping. Failure/unknown
semantics and the task-finished event's normalized-omission summary remain
separate and unchanged.

Positive run output preserves the task-ID reply and emits the experimental
cancellation warning. Launch acknowledgment is not completion. Cancellation
refuses before target lookup or effects. This subset never records full-control
`AcceptanceRecorded`; completion may use the existing `SpawnObserved` path only
after all terminal gates pass. Existing public status and task-event reads remain
the observation surface.

## Observed verification and blocker

Evidence is retained outside the product repository in workspace
`.claude-work/codex-2026-11-17/usable-mac-first-task/`.

- Final strict workspace/all-target Clippy passed in `first-run-clippy-r5.log`
  after the test-only corrections. Formatting, architecture and legacy guards
  also passed.
- `first-run-tests-r5.log` records 41 passing focused tests across six targets.
  `first-run-summary-tests-r3.log` records passing host stream (6), interface
  terminal-summary (9), and interface runtime-contract (13) tests.
- The full workspace attempt in `first-run-workspace-tests-r1.log` completed
  with 2299 passing tests and one failure: snapshot test fixture initialization
  returned `Busy`. The test-only correction gives initial fixture setup a
  10-second deadline; it does not change the tested snapshot's contention budget.
  After that correction, all 197 storage library tests passed in 20.16 seconds
  (`first-run-storage-lib-r2.log`). The complete workspace was not repeated after
  this correction; the failed attempt and final affected-target rerun are both
  retained. A subsequent strict Clippy pass is not yet claimed.
- The first normal release artifact, SHA-256
  `f9b01e06cc16c9ae390062165bb1e184035dcb9be3af243ea5bd8c46080b433a`,
  passed the selected refusal oracle. Its complete fake case refused before fake
  registration and before creation of the case state root. Cleanup reported no
  remaining participants. `oracle-first-run-r1/report.json` retains the failed
  complete case; neither actual task completion nor the frozen six-case lifecycle
  acceptance was demonstrated.

The OS-enabled Python black-box run recorded 951 tests with one stale test failure,
two skips and three expected failures. The failing marker test still replaced
schema 7 literally after the schema had advanced to 8. It now derives the current
version from the owning source, asserts the mutation changes the marker, and
retains the previous-version refusal assertion. All 32 fixture-seed module tests
passed afterward (`first-run-python-seed-r4.log`). The entire Python suite was
not rerun after this test-only correction; this is not a full compatibility or
lifecycle acceptance claim.

Read-only diagnostics localized a concrete pre-root caller-admission blocker:
PID 1 `proc_pidinfo(PROC_PIDTBSDINFO)` returned EPERM, including with requested
OS access, while own and parent observations succeeded. Unknown ancestry still
refuses mutation. The external `caller-anchor-alternatives.md` records observed
kernel-query alternatives and their unresolved safe-API, allocation/deadline,
ABI and bookend gates. No alternative caller anchor was adopted or qualified.

Remaining acceptance requires a reviewed bounded caller-observation route and
the independently pinned normal release fake oracle. Real-provider execution,
full-control acceptance, cancellation, adoption, release and Mac integration
remain separate gates.

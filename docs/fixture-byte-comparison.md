# Fixture byte comparison

`tests/test_ctl_fixture_bytes.py` runs the real `harflow-ctl` over roots seeded with `hf-fixture-seed` and compares its output with every fixture that `tests/fixtures/contracts-v1.provenance.json` classifies as `added` since 0.0.11 (43 fixtures). Each fixture is either compared or excluded with a reason; the test fails if a fixture is in neither set or in both, or if this document stops listing a substitution or an exclusion.

Policy (plan R3-D): inputs are deterministic where the seed port allows (ids, stamps, notes, repository strings, generation, titles are the generators' values); the substitutions below are the only permitted differences; every other byte, compared as canonical compact JSON (sorted keys, `ensure_ascii` off, trailing newline), must equal the fixture. The raw stdout is also required to be the reference spelling (`json.dumps`, sorted keys). Each substitution first asserts the Rust value it replaces.

## Substitutions

| Name | Reason |
| --- | --- |
| `product_version` | Rust advertises 0.0.13+rust; the fixture says 0.0.13. |
| `observed_at` | Reader-clock stamps: the Rust reader stamps the real clock (checked as a recent UTC stamp); the fixture carries the fixed instant of the Python clock. |
| `pending_cursor` | Opaque pending cursor: decoded and round-tripped; its root digest and identity binding (store generation, lexical root) are replaced by the fixture's. |
| `artifact_cursor` | Opaque artifact cursor: decoded and round-tripped; its revision (a random per-store token, checked against the stored manifest revision, not the reference's content digest) is replaced by the fixture's. |
| `root_path` | The scratch root and repository live under a temporary directory; that prefix is written /workspace, as the Python generators did. |
| `artifact_path` | The port places every artifact at <repo>/out/<name>; each path is replaced by the fixture's path for the same artifact id (the Python engine-output path is in its scratch directory). |
| `artifact_observation` | Present targets are real files: their registered device, inode, size and mtime_ns are the real stat, replaced by the fixture's stat constants. |
| `status_subset` | Task bodies are compared on the exact key intersection; the fixture keys Rust does not emit (SEED_ABSENT) and the Rust keys the fixture lacks (RUST_ONLY) are asserted as exact enumerated sets and removed before the byte comparison. |
| `session_id` | Known difference: Rust cannot represent a null session id (the store column is NOT NULL, TaskFacts.session_id is a String), so a task without one reports "" where Python reports null. |

Cursors are additionally checked before substitution: re-encoding the decoded cursor reproduces it exactly (compact sorted JSON for pending, `json.dumps` default spacing for artifacts); the pending root is the SHA-256 of the lexical state root and its identity is the store generation as four 32-bit integers; the artifact revision is a random 32-hex per-store token (not the reference's content digest) equal to the revision stored for the task. The follow-up request with the original cursor must return the remaining items (pending: task `5`x32, `has_more` false; artifacts: ids 3 and 4, `has_more` false).

## Known wire differences from Python status

The five continuation-stop fixtures are `status`, `status --brief` and `status-batch` documents. Their task objects are compared on the exact key intersection (every shared value equal, `continuation_stop` observed_at aside). The key sets that differ are enumerated and asserted exactly in `bytes_support.py` (fixture keys missing from Rust must be `SEED_ABSENT` plus `RUST_UNPROJECTED`; Rust keys missing from the fixture must be `RUST_ONLY`).

Seed limit only (not a Rust difference): `SEED_ABSENT` are stored record data that Rust serves verbatim from its blobs, but the seed port writes no value for them.
- `status` and `status-batch` (21): `allowed_commands`, `auto_continue`, `base_commit`, `caller_approvals`, `caller_write_approvals`, `depth`, `dispatch_origin`, `enforcement`, `extra_path_certification`, `extra_writable_paths`, `interaction_owner`, `lineage_detected`, `live_input`, `max_depth`, `max_turns`, `outcome`, `parent_task_id`, `reasoning_effort`, `root_task_id`, `spawned_by`, `start_dirty`
- `status --brief` (3): `dispatch_origin`, `parent_task_id`, `root_task_id`

Real projection differences, for the owner:
- `RUST_ONLY` (keys Rust emits that Python's rows lack, same for all three commands): `finished_at`, `needs_reconciliation`, `observed_exit`, `persisted_status`, `process_identity_state`.
- `session_id`: Rust cannot represent a null session id (`TaskFacts.session_id` is a String and the schema column is NOT NULL), so it emits `""` where Python emits null. Covered by the `session_id` substitution.
- `RUST_UNPROJECTED` (31 keys for `status` and `status-batch`; the `status --brief` subset is `background_idle_seconds`, `input_channel`, `last_captured_output_at`, `note`, `observation_note`, `output_capture`, `owned_by_live_server`, `responding_server`, `supervision`): Python values that are read-time projections, which Rust does not project:
  - `approval_summary`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `available_tool_count`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `background_idle_seconds`: resolved from configuration at read time; Rust has no projection.
  - `denial_categories`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `events_log`: path derived from the Python log directory; the Rust store has no such file.
  - `failure_diagnostic`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `input_channel`: read-time observation of the reader or live owner; Rust has no projection.
  - `is_error`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `last_captured_output_at`: read-time observation of the reader or live owner; Rust has no projection.
  - `last_output_tail`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `mcp_servers`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `messages`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `metadata_persisted`: constant added by the Python read path; Rust has no such member.
  - `note`: Python's supervision text; Rust removes a stored note (task_docs.rs) and uses its own classifier, which gives no note for a completed task.
  - `notices`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `num_turns`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `observation_note`: Python's observer-continuity text; Rust gives its own text only when a parent observer is known (read_docs.rs).
  - `output_capture`: read-time observation of the reader or live owner; Rust has no projection.
  - `owned_by_live_server`: read-time observation of the reader or live owner; Rust has no projection.
  - `pending_messages`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `permission_denials`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `queued_followup_count`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `queued_followups`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `raw_stream_log`: path derived from the Python log directory; the Rust store has no such file.
  - `recovery_activity`: read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection.
  - `responding_server`: identity of the responding Python process; Rust has no such process identity.
  - `resume_command`: derived from backend, session and path authority at read time; Rust has no projection.
  - `summary`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `supervision`: read-time observation of the reader or live owner; Rust has no projection.
  - `total_cost_usd`: parsed from the raw stream log at read time; Rust has no stream-log projection.
  - `usage`: parsed from the raw stream log at read time; Rust has no stream-log projection.

## Excluded fixtures

| Fixture | Reason |
| --- | --- |
| `ctl-continue-chain-stopped.json` | needs task-continue.v1 (E2 write) |
| `ctl-launch-continue-accepted.json` | needs task-continue.v1 (E2 write) |
| `ctl-launch-continue-refused.json` | needs task-continue.v1 (E2 write) |
| `ctl-launch-continue-unknown.json` | needs task-continue.v1 (E2 write) |
| `ctl-task-artifacts-preparing.json` | ownership_preparing (workflow ownership catalog bootstrap) is not produced by Rust (E6) |
| `ctl-task-cancel-incomplete.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-partial.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-pending-child-withdrawal-unsettled.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-pending.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-refused-documents.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-settled.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-truncated.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-unknown.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-cancel-unrecorded.json` | needs task-cancel-cascade.v1 (E2 write) |
| `ctl-task-pending-bound.json` | page-level namespace_bound is unreachable through the binary (the candidate check wins first, recorded ordering delta in docs/pending-launch.md); its document is pinned at interface level by hf-interfaces/tests/ctl_pending_docs.rs |
| `invalid-s05-missing-task-artifacts.json` | negative version-info document that no command produces |
| `invalid-s05-missing-task-cancel-cascade.json` | negative version-info document that no command produces |
| `invalid-s05-missing-task-continuation-stop.json` | negative version-info document that no command produces |
| `invalid-s05-missing-task-continue.json` | negative version-info document that no command produces |
| `invalid-s05-missing-task-pending-launch.json` | negative version-info document that no command produces |

## Compared fixtures

| Fixture | Substituted fields | Comparison |
| --- | --- | --- |
| `ctl-task-pending-empty.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-states.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-refused.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-status-pending.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-status-recorded.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-status-not-found.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-status-unknown.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-status-refused.json` | `product_version`, `observed_at` (`result.observed_at`) | exact |
| `ctl-task-pending-next-page.json` | `product_version`, `observed_at`, `pending_cursor` (`result.next_cursor`) | exact |
| `ctl-task-pending-cursor-invalid.json` | `product_version` | exact |
| `ctl-task-pending-existing-status-pending.json` | `product_version` | exact |
| `ctl-task-pending-existing-batch-pending.json` | `product_version` | exact |
| `ctl-task-artifacts-empty.json` | `product_version` | exact |
| `ctl-task-artifacts-unavailable.json` | `product_version` | exact |
| `ctl-task-artifacts-error.json` | `product_version` | exact |
| `ctl-task-artifacts-page-error.json` | `product_version` | exact |
| `ctl-task-artifacts-populated.json` | `product_version`, `observed_at` (each item availability), `root_path`, `artifact_path`, `artifact_observation` (items 1 and 2), `artifact_cursor` | exact |
| `ctl-task-artifacts-next-page.json` | `product_version`, `observed_at`, `root_path`, `artifact_path`, `artifact_observation` (item 4) | exact |
| `ctl-task-status-continuation-not-stopped.json` | `product_version`, `observed_at` (`continuation_stop`), `status_subset`, `session_id` | partial (task bodies on the key intersection) |
| `ctl-task-status-continuation-stopped.json` | `product_version`, `observed_at` (`continuation_stop`), `status_subset`, `session_id` | partial (task bodies on the key intersection) |
| `ctl-task-status-continuation-unknown.json` | `product_version`, `observed_at` (`continuation_stop`), `status_subset`, `session_id` | partial (task bodies on the key intersection) |
| `ctl-task-status-brief-continuation-stopped.json` | `product_version`, `observed_at`, `status_subset`, `session_id` | partial (task bodies on the key intersection) |
| `ctl-task-status-batch-continuation-stop.json` | `product_version`, `observed_at`, `status_subset`, `session_id` | partial (`tasks[0]` on the key intersection; `errors` exact) |

The fixture ids, stamps and strings the seeds reproduce are in `tests/bytes_pending.py`, `tests/bytes_artifacts.py` and `tests/bytes_stop.py`. The pending cursor's follow-up request and the artifact cursor's follow-up request prove that each original cursor resumes the walk.

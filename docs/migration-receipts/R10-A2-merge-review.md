# R10-A2 merge-gate review (A9-A11)

2026-11-18. Fresh review of R10-A2 (`8c542df` A9, `14d6275` A10, `73af64e` A11,
merge base `b6057b2`) before merging branch `claude/r10-workflow-definitions-e2f0c3`
into `main`. The earlier A9-A11 review record named no reviewers, listed no
rejected findings and had no plan review for A10/A11; this record stands on its own.
Decisions cited below (prefixed "R10-A" to distinguish them from other plans' numbering) are in [R10-A decisions](../decisions/R10-A-workflow-definitions.md).

Merge: `55826c5` (merge commit, no rebase). The branch and its worktree are left in
place.

## Reviewers

- Opus 5.5, in-process Claude Code `Agent` subagent, read-only, fresh context.
- Astra (`gpt-6-astra`, reasoning effort low), Codex CLI `codex exec -s read-only`,
  fresh thread `01a1ed41-3af6-7e32-82ef-15376bf49970`.

Both reviewed `git diff b6057b2..73af64e` and the three receipts against HarflowMCP
(fixture reference `14f0c1fe`, current `4cb9d32`). Neither ran cargo; Opus ran the
real Python `WorkflowStore.save` for the execution-mode case.

## Findings

| # | Reviewer | Severity | Finding | Decision | Reason |
| --- | --- | --- | --- | --- | --- |
| 1 | Astra | blocker | `save_defaults.rs:36` defaults a new resource-bearing agent to `headless`; Python defaults to `prefer_subagent` and refuses save. | Rejected | Owner-accepted divergence R10-A D6 (2026-11-15). The decision was not recorded in the repository, which is finding 3; recorded now and cited from the A10 receipt. |
| 2 | Astra, Opus | should-fix / nit | `workflow_snapshot_oracle.rs:14,49` iterate cases without asserting cardinality, so an empty fixture passes; the root `forced_definition_sha256 == definition_sha256` assertion is tautological. | Accepted | Assert exactly the four expected unique case IDs; drop the tautological root assertion, keep the pin forced-hash checks. |
| 3 | Opus | should-fix | Receipts rely on owner decisions held only outside the repository (P13/R10-A D4, R10-A D6, R10-A addendum item 2); the revision-carry message and its ordering relative to catalog errors differ from Python without a traceable exception. | Accepted | Added `docs/decisions/R10-A-workflow-definitions.md`; A9 and A10 receipts cite R10-A D4, R10-A D6 and R10-A addendum item 2, including the message and ordering divergence. |
| 4 | Opus | should-fix | No committed test guards the resolver and scope-validation memo bounds; removing the memo would go unnoticed. | Accepted | A deterministic work-count test for resolution (codec hash calls on a wide layered catalog) and a timing test for scope validation, which takes no port to count through. |
| 5 | Opus | nit | `launch_definition` (`snapshot.rs:52-56`) silently drops non-empty overrides when `orchestrator` is missing or not an object. | Accepted | Structured refusal when overrides are non-empty and the orchestrator is not an object. |
| 6 | Opus | nit | `remove_failure_keeps_source` (`workflow_store_failures.rs:100`) exercises only the `MemStore` double. | Accepted | Routed through `delete_definition`. |
| 7 | Opus | nit | `check_depth` and the cache-key `format!` run per occurrence before the memo check (`references.rs:78,93`). | Rejected | Work stays bounded and below Python's per-pin revalidation; not a correctness issue. |
| 8 | Opus | nit | `references.rs` is 294 substantive lines; `Resolution::enter` and `missing_references` are about 73-74 lines. | Rejected | Within the enforced maxima (400/80). |
| 9 | Opus | nit | `get_definition` qualifies twice (`catalog.rs:108`). | Rejected | Harmless; no behaviour change. |
| 10 | Opus | nit | `snapshot.json` provenance names the commit but no source SHA-256 (the tree fixture pins one). | Rejected | The commit pin is reproducible and the generators live outside the repository by policy. |

No plan review was run for these fixes separately: they are exactly the reviewers'
accepted remedies, and the follow-up code review below covers them.

## Follow-up fixes and verification

The fixes above were present as uncommitted work on merged `main` at `55826c5`
when this checkpoint resumed. No production changes were added during the final
verification pass.

### Retained follow-up review (round 2)

The 2026-11-18 Engine handover records completed Opus and Astra follow-up reviews:
Opus's three items were accepted, and the final test tightening was written but
not verified before handoff. The retained handover does not itemize all three
Opus findings, so this receipt does not reconstruct missing reviewer wording.
The identifiable remedies are the resolver lower bound plus exact count and the
widened scope fixture. Astra requested a production work counter in
`validate_tree_scopes`; the prior coordinator rejected this as a test-only hook
in production. This remains a timing regression test with the limitations below.

### Final follow-up review (round 3)

- Native Codex subagent `gpt-6-astra`, low effort, read-only, reviewed the complete
  tracked and untracked patch against `55826c5`. No actionable correctness
  findings. It confirmed the exact 274-hash calculation, override refusal,
  oracle cardinality and production delete-path coverage. All changes remain
  within the earlier accepted remedies. Its request to complete this receipt
  with actual results and identities was accepted.
- Claude Code, `claude-opus-5-5`, high effort, session
  `4ab135c9-4c81-4b85-a247-0554c1d3bca2`, reviewed the owner-authorized R10-A2
  packet with tools disabled. The result reported successful completion and Opus
  model usage. It found no blocking correctness issues and confirmed both work
  bounds, override refusal and oracle cardinality.

| Opus finding | Decision | Reason and remedy |
| --- | --- | --- |
| I1: Delete-failure test could pass on an earlier document error | Accepted | Assert the exact injected removal error and verify the source still exists. |
| I2: Override refusal needs a durable record and reachability check | Accepted | Save validation supplies an object when omitted and rejects non-objects. The A11 receipt now records the raw-root precondition, structured refusal and Python exception. |

Optional timing and fixture changes were deferred. Both local runs met the
existing threshold, and the exact-count fixture intentionally counts name-based
identity hashes. Separate scope and dependency tests cover deeper revisits and
resource context. Round 3 closes with these scoped remedies.

### Final verification

Environment: `CARGO_NET_OFFLINE=true`, `CARGO_BUILD_JOBS=2`,
`RUST_TEST_THREADS=2`,
`CARGO_TARGET_DIR=/Users/nayanda/.cache/harflow-engine/target`, and
`PYTHONDONTWRITEBYTECODE=1`. Cargo was added to `PATH` from this account's
`/Users/nayanda/.cargo/bin`. Checks run outside the sandbox with the authorized
per-account cache. Exact-name process preflight precedes each gate. One preflight
found an `xcodebuild` process after workspace check; verification paused until it
exited and coordination explicitly resumed the lane.

Both focused work-bound tests passed. The resolver observed exactly 274 identity
hashes. The widened scope test (301 distinct definitions, width 100, 1,000 leaf
nodes) completed in 0.11 s including fixture setup. The actual assertion remains
500 ms for the scope walk; the handover's reference to a 3-second limit was
inaccurate. The limit was not loosened. This local timing observation is not a
cross-machine performance or production qualification claim.

All eight development gates passed on merged `main` with the inherited fixes,
before the round-3 test assertion and documentation edits:

| Command | Observed result | Elapsed |
| --- | --- | --- |
| `cargo fmt --check` | PASS | 1.641 s |
| `cargo check --workspace --locked` | PASS | 14.996 s |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS | 21.801 s |
| `cargo test --workspace --locked` | 2,383 passed, zero failed, 65 ignored; 347 summaries | 418.457 s |
| `python3 scripts/check-architecture.py` | PASS | 1.607 s |
| `python3 scripts/check-legacy-names.py` | PASS | 0.190 s |
| `python3 -m unittest discover -s tests` | 965 run; OK, 2 skipped and 3 expected failures | 238.012 s |
| `cargo run --locked -p hf-host --bin harflow-engine-scaffold` | PASS | 1.936 s |

After the round-3 test tightening, formatting initially flagged assertion
wrapping. After applying that format, `cargo fmt --check`,
`cargo clippy -p hf-application --test workflow_store_failures --locked -- -D warnings`,
and `cargo test -p hf-application --test workflow_store_failures --locked`
all passed. The focused suite passed all four tests, including the exact
injected removal refusal and preservation of the source. Production code was
unchanged from the full gate.

The shared build/test lane was explicitly released after each batch. The final
exact-name process check returned no matches. Local evidence is retained in
workspace `.claude-work/codex-2026-11-18/r10-a2/`, including gate logs,
`gate-results.json`, `final-focused-results.json` and both final reviewer records.

This closes the local R10-A2 follow-up checkpoint. S4 repair and substitute
control planning are ready for a separate assignment.

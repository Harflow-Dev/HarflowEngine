# Rust engine parallel migration checklist

Revised 2026-11-04. Governing [revision 3](../../HARFLOW_MIGRATION_PLAN_V3.md),
[workspace gates](../../HARFLOW_MIGRATION_CHECKLIST.md) and [current sessions](migration-sessions.md).

Experimental scaffold remains unqualified. R01/R02+ implementation may proceed
parallel with M02+ native work after accepted M01 shared readiness. Both M12 and
R14 acceptance gate I01 integration; no early production cutover. Canonical contracts remain
HarflowMCP-owned until O04. Preserve accepted engine scope and unresolved decisions.

- [ ] R01 After shared readiness, approve process/SQLite/durability/responder/runtime/distribution seams and
  explicit scope/complete fault matrix; read-only research needs separate assignment.
- [ ] R02 Fresh bundled SQLite, durable bounded transactions/reads, capabilities,
  canonical compatibility and cursor generation; no shared Python state root.
- [ ] R03/R04 Detached owners, identity/boot/PID reuse, duplicate prevention,
  descendant cancel/drain/capture and crash/restart/disk-full/concurrency faults.
- [ ] R05–R07 Claude, continuation/messages, exact-action authenticated responder
  and native secure storage contract; never replay uncertain mutation outcomes.
- [ ] R08/R09 Resource/phase fairness/holder death, worktrees/scratch provenance
  and safe cleanup; exclusive resources across roots need explicit allocation.
- [ ] R10/R11 Workflow definitions/conflicts/snapshots, execution/nested/parallel/
  on-demand/orchestrator, attention/questions/recovery and cancellation guarantees.
- [ ] R12 Codex/Vibe/setup/MCP schemas and explicit unsupported refusals; real
  harness version/platform qualification requires separate authorization.
- [ ] R13 Statistics coverage/cost provenance; validated definitions/config/preferences
  import only, no task/run/event histories, live adoption or reverse conversion.
- [ ] R14 External-oracle conformance and full retained fault families; accepted
  Rust budgets and exact runtime/platform identities; missing evidence disposed.
- [ ] I01–I04 Native Mac integration and independent artifact/distribution checks.
- [ ] O01–O04 Cutover rehearsal/backup/archive/registration handoff, observation and
  explicit contract transfer/Python retirement. HarflowMac is retained.
- [ ] W02 Later Linux engine/server qualification; WIN02 Windows job/process/
  filesystem/authority/backend qualification only after Web stable owner gate.

Use existing Cargo architecture/dependency/check guidance. Never claim stock backend,
managed runtime or platform qualification from synthetic S07–S09 profiles.

## Product engine API

The native app uses **harflow-ctl as its engine API**. MCP is agent-facing only:
no native MCP client or fallback. Existing harflow-setup acquisition/registration
tasks remain separate from product operations. If required app functionality lacks
a ctl operation, first scope additive work in the current canonical contract owner:
HarflowMCP before accepted O04 transfer, HarflowEngine afterward. Specify provenance,
authority, bounds and unknown/refusal semantics, capability, fixtures and tests;
then coordinate byte-identical fixture vendoring and consumer adapters. Pre-transfer
native work starts in Python; Rust implements that same accepted ctl contract.
After transfer, later Web work does not depend on retired Python. Missing API is not
permission to read persistence, invoke agent MCP or invent semantics. M03 may split
bounded suffix sessions.

## Parallel-track coordination

M01 shared readiness follows S09 coverage disposition and the native capability,
code/private-read inventory, with ratified initial ctl/capability/fixture baseline,
exact identities, explicit blocking gaps and canonical change owner. M02+ native
and R01/R02+ Rust work may then run concurrently with disjoint files/isolated roots.
Pin each accepted subset, serialize canonical changes under MCP before O04 (Engine
afterward), coordinate consumer fixture deltas and carry forward affected conformance.
Both accepted M12 and R14 are required before I01; missing baseline/fault evidence
is not automatically waived. Read-only research remains separately assignable.

## Boundaries and evidence

This plan authorizes no implementation by itself. Follow workspace/child/package
AGENTS.md and the current session brief; preserve concurrent work in primary
checkouts. Use isolated roots/fixtures. No implicit live-state, install, registration,
paid-probe, signing, push, publication, retirement or deletion authority. Record exact
commit/contract/design/runtime/profile identities, actual checks/reviews, missing
coverage and owner decisions in the lead receipt. Delivery and acceptance differ.
Builds/tests must not depend on sibling source checkouts. Documentation-only plan
changes need link/consistency checks, not product test reruns.

## Conversational steering scope amendment — 2026-11-05

ST06 implements the canonical steering state machine and public ctl behavior as part of affected R11/R14 conformance, before integration. O04 transfer is not a prerequisite for consuming accepted contracts. Current Rust work continues unchanged; no ST06 implementation is assigned.
See the [current ST session briefs](../../HARFLOW_MIGRATION_SESSIONS.md#conversational-steering-track--accepted-scope-2026-11-05)
and [steering plan](../../HARFLOW_WORKFLOW_STEERING_PLAN.md). Scope acceptance does
not pass prior gates, permit live control or authorize releases.

# HarflowEngine migration sessions — native revision 3

Revised 2026-11-04. The [workspace session index](../../HARFLOW_MIGRATION_SESSIONS.md)
is authoritative for each brief, prerequisites, acceptance and receipt output.
Read [repository checklist](migration-plan.md) and applicable guidance before work.

## Current ownership

R01–R14, O01, I01–I04 co-owned, W02 and WIN02 later.

- **R01 — Rust architecture and distribution decisions**; prerequisites and acceptance in the workspace index.
- **R02 — Rust store and public reads**; prerequisites and acceptance in the workspace index.
- **R03 — Process primitives and ownership**; prerequisites and acceptance in the workspace index.
- **R04 — Recovery and durability faults**; prerequisites and acceptance in the workspace index.
- **R05 — Claude backend**; prerequisites and acceptance in the workspace index.
- **R06 — Continuation and messages**; prerequisites and acceptance in the workspace index.
- **R07 — Approval responder and authority**; prerequisites and acceptance in the workspace index.
- **R08 — Slots and resource fairness**; prerequisites and acceptance in the workspace index.
- **R09 — Worktrees and shared scratch**; prerequisites and acceptance in the workspace index.
- **R10 — Workflow definitions**; prerequisites and acceptance in the workspace index.
- **R11 — Workflow execution and recovery**; prerequisites and acceptance in the workspace index.
- **R12 — Codex, Vibe, setup and MCP**; prerequisites and acceptance in the workspace index.
- **R13 — Statistics and validated import**; prerequisites and acceptance in the workspace index.
- **R14 — Rust conformance acceptance**; prerequisites and acceptance in the workspace index.
- **O01 — Owner cutover rehearsal**; prerequisites and acceptance in the workspace index.
- **WIN02 — Windows engine qualification**; prerequisites and acceptance in the workspace index.

Each assignment is bounded and stops before the next. Record its lead receipt in
`docs/migration-receipts/<ID>.md`; co-owned work is included in that lead receipt.
No plan status authorizes live operations or publication. All new sessions are planned.

## Historical session evidence

S01–S09 keep their original IDs, outputs and recorded limits. S09 bounded evidence
is delivered, not accepted. Historical future S10–S50/X01–X03 are superseded and cannot be dispatched;
see the [crosswalk](../../HARFLOW_MIGRATION_PLAN_V3.md#supersession-and-scope-crosswalk).
The preserved briefs below describe original scope, not new authorization.

### S03 — Freeze, parity and scope decisions

- Status: planned. Lead: HarflowMCP. Co-owned: HarflowMac, HarflowDesign, HarflowEngine, HarflowWeb.
- Prerequisite receipts: S02.
- Checklist coverage: B0-MC-02 (final contribution), B0-MA-04 (final contribution), B0-DS-01 (final contribution), C0-DS-04 (partial), B0-EN-01 (final contribution), B0-WE-01 (partial).
- Scope and acceptance: Record Keep, Simplify, Deliberately remove and Undecided decisions in the v2 coverage ledger; obtain decisions on freeze, representative datasets and performance budgets, preserving unresolved scope and approved removals.
- Output: lead receipt `docs/migration-receipts/S03.md`, with the evidence and identities required by the dispatch protocol.

## Conversational steering scope amendment — 2026-11-05

ST06 implements the canonical steering state machine and public ctl behavior as part of affected R11/R14 conformance, before integration. O04 transfer is not a prerequisite for consuming accepted contracts. Current Rust work continues unchanged; no ST06 implementation is assigned.
See the [current ST session briefs](../../HARFLOW_MIGRATION_SESSIONS.md#conversational-steering-track--accepted-scope-2026-11-05)
and [steering plan](../../HARFLOW_WORKFLOW_STEERING_PLAN.md). Scope acceptance does
not pass prior gates, permit live control or authorize releases.

# HarflowEngine migration sessions

Status: planned. The [workspace session index](../../HARFLOW_MIGRATION_SESSIONS.md) defines dispatch order; the [repository checklist](migration-plan.md) retains full item criteria. These session specifications include work led or co-owned by this repository.

## Dispatch protocol

Read the workspace `AGENTS.md` and `HARFLOW_MIGRATION_CHECKLIST.md`, the lead and co-owned repositories' `AGENTS.md` files where present, their `docs/migration-plan.md` files, applicable package guidance, and the session and all accepted prerequisite receipts before work. The workspace session index gives the full sequence. A session is a bounded assignment, not an instruction to start the next session. A standalone session lead owns the normal required plan/code reviews, test delegation and validation; a worker explicitly handed off by an orchestrator follows that handoff and does not restart its workflow. Stop and report a concrete unresolved ambiguity or missing prerequisite instead of inventing a decision.

Use primary checkouts and preserve concurrent work. Never share a writable state root between engines or import historical task/run/event state. Work on isolated roots and fixtures unless separately authorized. MCP owns canonical contracts until explicit S50 transfer. Cross-repository package changes require a committed producer, exact consumer pin and clean install without sibling source dependencies; a necessary push requires current authorization. Do not run concurrent sessions that mutate the same repository, generated contract bundle or IPC boundary. Within a session, coordinate delegated workers through explicit file ownership. Approval for publication, signing, paid probes, live state, registrations, cutover and retirement is separate and current; prepare concrete evidence first.

A session receipt is proposed at the lead repository's `docs/migration-receipts/<session>.md`. It records prerequisites, accepted scope and decisions, exact commits for every involved repository, contract/design/package/runtime identities, actual commands and results, platform and profiles, acceptance evidence, deviations, missing checks and authorization or blockers. Distinguish local, committed, pushed, CI-passed, packaged and released states. It contains technical evidence, not review transcripts. A multi-repository session has one lead receipt covering each repository. A receipt is accepted only after the applicable review and owner decisions. All sessions here are planned, not accepted.

Checklist items may span sessions. Each mention is partial until the final contributing session identified below; even then, check the source item only after all contributing receipts and the full original item acceptance have been reviewed and accepted. The sequential order is the safe default; S25's isolated research exception and optional sessions are explicit. There is no duration guarantee. Defects or follow-up work discovered during a session need separately scoped assignments; they do not automatically expand or advance the session.

Copyable dispatch (replace bracketed inputs):

```text
Run session Sxx from HARFLOW_MIGRATION_SESSIONS.md and the lead repository's docs/migration-sessions.md.
Accepted prerequisite receipt paths and IDs: [paths and IDs]
Pinned contract, design, package and runtime identities: [commits, digests and paths]
Allowed isolated roots, fixtures and external actions: [scope and authorizations]
Return the session receipt at the specified lead-repository path, including all repository commits, evidence, decisions and blockers. Complete only this session; do not start the next.
```

For optional work replace `Sxx` with `X01`, `X02` or `X03`.

## Sessions

### S03 — Freeze, parity and scope decisions

- Status: planned. Lead: HarflowMCP. Co-owned: HarflowMac, HarflowDesign, HarflowEngine, HarflowWeb.
- Prerequisite receipts: S02.
- Checklist coverage: B0-MC-02 (final contribution), B0-MA-04 (final contribution), B0-DS-01 (final contribution), C0-DS-04 (partial), B0-EN-01 (final contribution), B0-WE-01 (partial).
- Scope and acceptance: Obtain owner decisions on freeze, parity, scope, representative datasets and performance budgets; inventory design approvals, engine scaffold and deferred Web; record accepted baselines and unknowns.
- Output: lead receipt `docs/migration-receipts/S03.md`, with the evidence and identities required by the dispatch protocol.

### S25 — Engine architecture decisions

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S03; may run earlier than S24 as isolated research only.
- Checklist coverage: C0-EN-01 (final contribution), C0-EN-02 (final contribution).
- Scope and acceptance: Run isolated spikes and secure owner decisions on process, storage, authority and dependencies; record platform evidence and unsupported cells. May run earlier only as isolated research.
- Output: lead receipt `docs/migration-receipts/S25.md`, with the evidence and identities required by the dispatch protocol.

### S26 — Rust store and public reads

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S24 (accepted E0) and S25.
- Checklist coverage: R0-EN-01 (final contribution), R0-EN-02 (final contribution).
- Scope and acceptance: After accepted E0, deliver bundled SQLite on fresh roots, durable transactions, bounded reads, backup, version and compatible errors.
- Output: lead receipt `docs/migration-receipts/S26.md`, with the evidence and identities required by the dispatch protocol.

### S27 — Rust process primitives

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S26.
- Checklist coverage: R0-EN-03 (partial).
- Scope and acceptance: Deliver detached ownership, process identity and tree termination, capture and drain with isolated evidence.
- Output: lead receipt `docs/migration-receipts/S27.md`, with the evidence and identities required by the dispatch protocol.

### S28 — Rust task ownership

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S27.
- Checklist coverage: R0-EN-04 (partial), R0-EN-09 (partial).
- Scope and acceptance: Implement task launch, resume, cancellation and session ownership; prove external fake-harness spawn behavior and duplicate prevention.
- Output: lead receipt `docs/migration-receipts/S28.md`, with the evidence and identities required by the dispatch protocol.

### S29 — Rust recovery faults

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S28.
- Checklist coverage: R0-EN-03 (final contribution), R0-EN-09 (partial), R0-EN-14 (partial).
- Scope and acceptance: Exercise restart, uncertain outcomes, duplicate reservations, owner death, PID reuse and disk-full recovery against external process/capture evidence.
- Output: lead receipt `docs/migration-receipts/S29.md`, with the evidence and identities required by the dispatch protocol.

### S30 — Claude adapter

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S29.
- Checklist coverage: R0-EN-07 (partial).
- Scope and acceptance: Implement and qualify the Claude adapter against an identified profile; real CLI use or spend requires separate authorization.
- Output: lead receipt `docs/migration-receipts/S30.md`, with the evidence and identities required by the dispatch protocol.

### S31 — Rust message continuation

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S30.
- Checklist coverage: R0-EN-04 (partial), R0-EN-10 (partial).
- Scope and acceptance: Implement supported v1 queued continuation and report uncertain effects without automatic retry; defer automatic continuation enhancements.
- Output: lead receipt `docs/migration-receipts/S31.md`, with the evidence and identities required by the dispatch protocol.

### S32 — Rust approval responder

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S31.
- Checklist coverage: R0-EN-04 (final contribution), R0-EN-11 (partial).
- Scope and acceptance: Implement authenticated responder protocol and secure storage port; refuse stale or duplicate decisions; defer real Desktop IPC proof to S42.
- Output: lead receipt `docs/migration-receipts/S32.md`, with the evidence and identities required by the dispatch protocol.

### S33 — Slots and resource fairness

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S32.
- Checklist coverage: R0-EN-05 (partial), R0-EN-13 (partial).
- Scope and acceptance: Implement slot phases and resource queues; verify fairness and holder death without treating uncertainty as release.
- Output: lead receipt `docs/migration-receipts/S33.md`, with the evidence and identities required by the dispatch protocol.

### S34 — Worktrees and scratch

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S33.
- Checklist coverage: R0-EN-05 (final contribution).
- Scope and acceptance: Implement worktrees and shared scratch with safe ownership and cleanup refusal for unsettled use.
- Output: lead receipt `docs/migration-receipts/S34.md`, with the evidence and identities required by the dispatch protocol.

### S35 — Workflow definitions

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S34.
- Checklist coverage: R0-EN-06 (partial), R0-EN-12 (partial).
- Scope and acceptance: Implement definitions, validation and builder interfaces; preserve content on invalid saves and conflicts.
- Output: lead receipt `docs/migration-receipts/S35.md`, with the evidence and identities required by the dispatch protocol.

### S36 — Workflow execution

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S35.
- Checklist coverage: R0-EN-06 (partial), R0-EN-12 (partial).
- Scope and acceptance: Implement scheduling, supervisor, joins and nested Parallel; use fakes for dependencies not implemented yet and verify sibling preservation.
- Output: lead receipt `docs/migration-receipts/S36.md`, with the evidence and identities required by the dispatch protocol.

### S37 — Workflow attention and recovery

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S36.
- Checklist coverage: R0-EN-06 (partial), R0-EN-11 (partial), R0-EN-12 (partial).
- Scope and acceptance: Implement questions, checkpoints, permission failure and recovery; refuse stale decisions and retain sibling state.
- Output: lead receipt `docs/migration-receipts/S37.md`, with the evidence and identities required by the dispatch protocol.

### S38 — Codex and Vibe adapters

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S37.
- Checklist coverage: R0-EN-07 (partial).
- Scope and acceptance: Qualify current supported Codex and Vibe transports and capability profiles with authorized real probes; do not require a new ACP feature.
- Output: lead receipt `docs/migration-receipts/S38.md`, with the evidence and identities required by the dispatch protocol.

### S39 — Setup and MCP

- Status: planned. Lead: HarflowEngine. Co-owned: HarflowMCP.
- Prerequisite receipts: S38.
- Checklist coverage: R0-EN-07 (final contribution).
- Scope and acceptance: Implement setup, registrations and included MCP tool schemas; verify public behavior with isolated configuration.
- Output: lead receipt `docs/migration-receipts/S39.md`, with the evidence and identities required by the dispatch protocol.

### S40 — Statistics and migration data

- Status: planned. Lead: HarflowEngine. Co-owned: None.
- Prerequisite receipts: S39.
- Checklist coverage: R0-EN-06 (final contribution), R0-EN-13 (partial), I0-EN-03 (partial).
- Scope and acceptance: Implement statistics and retention, honest incomplete accounting, validated definition/configuration import and backup/restore on isolated copies.
- Output: lead receipt `docs/migration-receipts/S40.md`, with the evidence and identities required by the dispatch protocol.

### S41 — Rust conformance acceptance

- Status: planned. Lead: HarflowEngine. Co-owned: HarflowMCP.
- Prerequisite receipts: S40.
- Checklist coverage: R0-EN-08 (final contribution), R0-EN-09 (final contribution), R0-EN-10 (final contribution), R0-EN-11 (final contribution), R0-EN-12 (final contribution), R0-EN-13 (final contribution), R0-EN-14 (final contribution), R0-MC-01 (final contribution).
- Scope and acceptance: Pass Python-equivalent conformance and all listed fault families on macOS with external oracles; adjudicate discrepancies and accept R0 without claiming other platforms.
- Output: lead receipt `docs/migration-receipts/S41.md`, with the evidence and identities required by the dispatch protocol.

### S42 — Rust adapter and responder IPC

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowClient, HarflowEngine.
- Prerequisite receipts: S41.
- Checklist coverage: I0-DE-01 (partial), I0-DE-04 (final contribution), R0-CL-01 (final contribution).
- Scope and acceptance: Select Rust on isolated state, reuse client and contract tests, and verify authenticated responder and secure credentials across real IPC.
- Output: lead receipt `docs/migration-receipts/S42.md`, with the evidence and identities required by the dispatch protocol.

### S43 — Offline managed engines

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowEngine.
- Prerequisite receipts: S42.
- Checklist coverage: I0-DE-02 (partial).
- Scope and acceptance: Stage artifacts without publication; qualify offline installation, immutable generations, interrupted installation and recovery.
- Output: lead receipt `docs/migration-receipts/S43.md`, with the evidence and identities required by the dispatch protocol.

### S45 — Integrated Rust acceptance

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowClient, HarflowDesign, HarflowEngine.
- Prerequisite receipts: S44.
- Checklist coverage: I0-DE-01 (final contribution), I0-DE-03 (partial), I0-CL-01 (final contribution), I0-DS-01 (final contribution), I0-EN-01 (final contribution).
- Scope and acceptance: Re-run full Rust-backed macOS acceptance, production pixels, accessibility, performance, clean-user install and upgrade; verify no hidden Python or private store coupling.
- Output: lead receipt `docs/migration-receipts/S45.md`, with the evidence and identities required by the dispatch protocol.

### S46 — Cutover rehearsal

- Status: planned. Lead: HarflowEngine. Co-owned: HarflowMac, HarflowDesktop, HarflowMCP.
- Prerequisite receipts: S45.
- Checklist coverage: I0-EN-03 (final contribution), O0-EN-02 (partial), O0-MA-01 (final contribution), O0-MA-02 (partial), O0-MC-01 (partial).
- Scope and acceptance: Rehearse isolated archive, restore, validated definition/configuration/preference imports, registration backup and handoff; document rollback boundaries, exclusive roots and no history import.
- Output: lead receipt `docs/migration-receipts/S46.md`, with the evidence and identities required by the dispatch protocol.

### S47 — Owner cutover and canaries

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowEngine, HarflowMac, HarflowMCP.
- Prerequisite receipts: S46.
- Checklist coverage: O0-EN-02 (final contribution), O0-MA-02 (partial), O0-MC-01 (partial).
- Scope and acceptance: Only with explicit owner authorization, settle active work, perform cutover and canaries; retain any qualified early-use next store and record first writes and external effects.
- Output: lead receipt `docs/migration-receipts/S47.md`, with the evidence and identities required by the dispatch protocol.

### S48 — Observation and go/no-go

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowEngine, HarflowMac.
- Prerequisite receipts: S47.
- Checklist coverage: O0-DE-01 (partial), O0-MA-02 (partial).
- Scope and acceptance: Complete owner-agreed observation period and restart/upgrade checks; record elapsed time and evidence for release decision.
- Output: lead receipt `docs/migration-receipts/S48.md`, with the evidence and identities required by the dispatch protocol.

### S49 — Publication

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowEngine.
- Prerequisite receipts: S48.
- Checklist coverage: O0-DE-01 (final contribution).
- Scope and acceptance: Prepare a concrete release, download and feed plan; resolve website ownership separately, obtain explicit publication authorization, then publish and verify qualified artifacts. No hidden website build or Web triggers.
- Output: lead receipt `docs/migration-receipts/S49.md`, with the evidence and identities required by the dispatch protocol.

### S50 — Contract transfer and retirement

- Status: planned. Lead: HarflowMCP. Co-owned: HarflowEngine, HarflowMac, HarflowDesktop, HarflowClient.
- Prerequisite receipts: S49.
- Checklist coverage: O0-MC-01 (final contribution), O0-EN-03 (final contribution), O0-MA-02 (final contribution).
- Scope and acceptance: With explicit owner decisions, transfer canonical contracts once with consumer/provenance updates and retire legacy product; cleanup needs separate authorization and never deletes user repositories implicitly.
- Output: lead receipt `docs/migration-receipts/S50.md`, with the evidence and identities required by the dispatch protocol.

### X01 — Linux qualification

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowDesign, HarflowEngine, HarflowClient.
- Prerequisite receipts: S45 (accepted macOS integration).
- Checklist coverage: I0-DS-02 (partial), I0-DE-03 (partial).
- Scope and acceptance: After integrated macOS acceptance, approve Linux design and independently qualify Linux x86_64 engine, backend and Desktop with native evidence before claiming support.
- Output: lead receipt `docs/migration-receipts/X01.md`, with the evidence and identities required by the dispatch protocol.

### X02 — Windows qualification

- Status: planned. Lead: HarflowDesktop. Co-owned: HarflowDesign, HarflowEngine, HarflowClient.
- Prerequisite receipts: S45 (accepted macOS integration) and X01 (Linux qualification).
- Checklist coverage: I0-DS-02 (final contribution), I0-DE-03 (final contribution).
- Scope and acceptance: After integrated macOS acceptance, approve Windows design and independently qualify Windows x86_64 engine, backend and Desktop with native evidence; does not block owner cutover.
- Output: lead receipt `docs/migration-receipts/X02.md`, with the evidence and identities required by the dispatch protocol.

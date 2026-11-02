# Rust engine migration checklist

Dispatchable work packages: [migration sessions](migration-sessions.md).

Status: planned, 2026-11-02. All work items remain unverified.

Own the experimental Rust engine and its independently testable layers.
HarflowMCP owns canonical contracts until explicit ownership transfer. Inputs
are the reviewed Python contract, conformance suite, parity ledger and accepted
E0 gate; outputs are Rust compatibility and cutover evidence.

## Dispatch and boundaries

Work only on the assigned item. Read the workspace AGENTS.md, this repository's AGENTS.md if present, and
applicable package instructions first; preserve concurrent edits and use primary checkouts.
The orchestrator delegates unit tests under the current workflow. A checkbox is
not authorization to start runtimes, mutate real roots, register clients, push,
release, clean up, or expand scope. Use disposable roots and workspaces where
needed. Report a blocker with concrete evidence; task status is not proof of
completion. Repository builds and tests must not depend on sibling checkouts.

The optional [workspace coordination index](../../HARFLOW_MIGRATION_CHECKLIST.md)
uses B0 (baseline), C0 (contracts and design), E0 (client and Electron on
Python), R0 (Rust parity), I0 (integrated stack), and O0 (owner cutover and
publication). It is not a build dependency. Freeze and qualify the reproducible
existing baseline without waiting for open feature tickets. Review v1 scope in a
parity ledger, including proposed sidebar additions, instead of assuming tickets
or UI polish are migration requirements without an accepted ledger decision. All boxes below are deliberately unchecked;
scaffolds alone do not establish completion. Early-use gates and defaults remain
subject to evidence and explicit authorization for root registration. Use fresh
Rust state; archive historical task, run and event records rather than importing
them. Import only definitions, configuration and preferences, with an explicit
registration backup and handoff. No engine may share a writable root.

Stage dependencies: B0 → C0 → E0 → R0 → I0 → O0. Isolated C0
architecture spikes do not depend on E0; production Rust implementation does.
Within R0, order work as store → lifecycle → messages, approvals, resources
and backends → workflows → statistics and integration.

## B0 / C0 — research and decisions

- [ ] B0-EN-01 Record scaffold commit and existing checks without claiming
      compatibility or production readiness.
- [ ] C0-EN-01 Run isolated architecture spikes for process identity, detached
      survival, termination, SQLite contention, transport and authority.
- [ ] C0-EN-02 Record platform evidence and an explicit owner decision on the
      process model; document unsupported cells and follow-up decisions.

## R0 — implementation after E0 acceptance

- [ ] R0-EN-01 Confirm the complete Electron-on-Python gate is accepted before
      production Rust implementation; spikes may precede this gate.
- [ ] R0-EN-02 Implement bundled SQLite on fresh roots, bounded reads, durable
      transactions, backups and contract-compatible errors and version info.
- [ ] R0-EN-03 Implement detached ownership, identity, process-tree termination,
      capture, drain and recovery; never automatically retry uncertain effects.
- [ ] R0-EN-04 Implement tasks, messages, approvals and responder authority.
- [ ] R0-EN-05 Implement slots, worktrees and shared scratch with safe cleanup.
- [ ] R0-EN-06 Implement workflows, parallel and nested Nodes, attention,
      recovery and statistics.
- [ ] R0-EN-07 Implement setup and MCP, and qualify Claude, Codex and Vibe with
      real CLI profiles when separately authorized; defer other backends.
- [ ] R0-EN-08 Pass fixture and black-box behavior conformance on isolated roots
      with external oracles; schema equality alone is insufficient.

## R0 — behavioral and fault acceptance

- [ ] R0-EN-09 Verify launch, resume, cancel and unknown outcomes with a fake
      harness spawn ledger; replay of the same operation identity or reservation
      cannot create duplicate execution.
- [ ] R0-EN-10 Verify queued, delivered, unknown and unsupported send states;
      uncertain external effects are not retried automatically.
- [ ] R0-EN-11 Verify stale or duplicate approvals and attention checkpoints are
      refused without applying an obsolete decision.
- [ ] R0-EN-12 Exercise invalid workflow saves and conflicts, nested and parallel
      recovery, preserving sibling Nodes and their state.
- [ ] R0-EN-13 Verify resource queue fairness and holder death; statistics report
      incomplete accounting rather than claiming a complete result.
- [ ] R0-EN-14 Test owner death, restart, kill and drain, PID reuse, disk full
      and recovery against external process and capture evidence.

## I0 / O0 — integration and cutover

- [ ] I0-EN-01 Revalidate the same accepted client against Rust and record
      differences, performance and platform-specific evidence.
- [ ] I0-EN-03 Rehearse archive and import of definitions, configuration and
      preferences only; exclude historical task, run and event state.
- [ ] O0-EN-02 Rehearse registration backup and explicit handoff; never allow
      Python and Rust to write one root or cut over active work.
- [ ] O0-EN-03 Transfer canonical contract ownership exactly once, only after
      explicit owner retirement decision and verified handoff.

## Validation

Run `cargo fmt --check`, `cargo check --workspace --locked`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo test --workspace --locked`, `python3 scripts/check-architecture.py`,
`python3 scripts/check-legacy-names.py` and `python3 -m unittest discover -s tests`
as appropriate; see [development](development.md) and [architecture](architecture.md).

## Handoff evidence

For every assigned item report: item ID and result; repository and dependency
commit IDs; fixture manifest hash where applicable; commands, environment,
results and evidence locations; local, committed, pushed, CI-passed, packaged
and released state, stated separately; approved design commit, path and state where
applicable; deviations, blockers and remaining work. Record unrun checks and
why. Do not infer passing gates from a dispatched task's status.

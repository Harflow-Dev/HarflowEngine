# R01 criterion and fault matrix

2026-11-04. Required future evidence, not test results. See
[architecture](R01-architecture.md) and [receipt](../migration-receipts/R01.md).
M01 B01–B18 retain their original meaning in the pinned supporting receipt;
this table assigns Rust follow-up without accepting or waiving any cell.
MCP owns producer semantics until O04; Engine owns Rust evidence; Mac owns native
consumer/platform evidence. Tracker serializes canonical deltas.

## Criterion to independent oracle and gate

| Obligation | Required scenario / acceptance criterion | Independent oracle and affected gate |
| --- | --- | --- |
| B01 launch | Missing binary/protocol init; crash before claim, after claim, after spawn and before ack. Never turn possible execution into refusal/replay. | External spawn ledger + OS child identity/exits + public result/captures; Engine R03/R04, qualified launch blocked until proven; MCP resolves baseline ambiguity. |
| B02 duplicate prevention | Simultaneous same-session claims and repeated operation IDs; changed payload refuses; uncertain reservation excludes another owner. | Barrier-controlled independent callers, spawn counts and durable identity correlation; R03/R04, later backend/continuation acceptance. |
| B03 identity/durability | Reused unrelated PID, reboot generation, unavailable identity and control race; commit/ack power-loss model. No false death or unsafe signal. | OS observations/handles, unrelated sentinel survivor, durable recovery snapshots; R03/R04/R14. Process-kill proof is separately labelled. |
| B04 containment/drain | Arbitrary trees, escaped session, pipe holder, full pipes, partial UTF-8 and owner loss. Separate exit, terminal publication and drained/capture evidence. | External descendant/exit watchers and byte hashes; R03/R04/R14. Unproved containment blocks claimed cancellation guarantee. |
| B05 delivery uncertainty | Lose response after queue/consume/dispatch commit; cancel-versus-consume race; fresh continuation variants. No automatic replay or duplicate effect. | Receiver-owned sentinel/byte ledger, public identity and capture correlation; R06 and affected R11 controls, not R02 pure reads. |
| B06 workflow/context | Nested forwarding/caller loss, decision/dispatch split, retry/fallback/on-demand, goals, complete acknowledged refs/cache; preserve issued siblings. | External spawned worker ledger, exact public checkpoint/input identities and controlled effects; R10/R11; canonical missing semantics MCP-first. |
| B07 responder | Concurrent answer, expiry/ownership transition, delivery loss; enrollment/rotation/revocation/wrong-store/locked storage; no stale or unauthorized effect. | Independent action sentinel + secure-store/access tests + public refusal/audit; R07/I01. Credential proposal requires owner/MCP adjudication first. |
| B08 enforcement | Exact stock backend freedom/network/native combinations and unsupported versions. API refusal is not OS confinement. | Out-of-policy filesystem/network effects attempted from explicitly authorized harness fixtures; backend slices/R14 and Mac M03/M11. |
| B09 resources | Phase/service fairness, capacity/borrow anchors, holder death, repair and unknown release. Root separation never releases cross-root resources. | External ordered spawn/effect ledger plus public capacity/identity; R08/R11/R14. Explicit exclusive allocation before shared test resources. |
| B10 scoped reads | Direct/worker/orchestrator/builder/cross-tree roles, prefix ambiguity and preparing/unavailable catalog; no pre-authority global counts or unauthorized candidates. | Independently constructed caller contexts and hidden sentinels; refusal/no-disclosure observations plus separate filesystem/cache tracing; R02 public-read qualification and later read slices. |
| B11 history/bounds | Generation/filter-bound cursors, replacement/pruning, missing/partial ownership, corrupt history, sparse filters and byte/row limits at CI/representative/stress scale. | Independent dataset model/page traversal/capture-size and timing/RSS measurements; R02 read acceptance, statistics R13 and R14. Small S09 paging is insufficient. |
| B12 backends | Stock loading, argv, stream quirks, real supported versions and capabilities. | Recorded/fake protocol checks first; separately authorized real-version/provider qualification; R05/R12/R14. Synthetic injection never becomes stock proof. |
| B13 reconnect/upgrade | Client disconnect/restart, immutable generation switch, unsupported schema, interrupted install and rollback limits. | Exact artifact identities, native adapter captures and clean-user packaged observations; Mac I01–I04, Engine compatibility input. |
| B14 worktree/scratch | Provision/remove/shared checkout, link identity/dirty/active guards, artifact/pin/retention combinations. | Disposable repositories and independent preserved/removed sentinels; R09/R14. Mutation-specific authorization required; native observation scope separate. |
| B15 definitions/builder | Cycles/depth/malformed records, expected revisions, proposal/draft conflicts/apply and unsaved work. | Independent definition inputs/public revisions and no-effect sentinels; R10/R11, MCP ctl-gap audit first; SB1-C not yet accepted. |
| B16 setup/control/recovery | Notifications/leases, per-client config edits/backups, privileged controls and fresh-store recovery. No implicit registration or legacy-state conversion. | Isolated configuration round trips, preserved unrelated bytes, authenticated refusals; R12/R13 and O01–O04 as applicable; unresolved scope remains owner-controlled. |
| B17 fault windows | Disk-full/I/O/corruption, MCP restart, cancellation/withdrawal, nested split, borrow/repair, notification leases, checkpoint/import interruption and active runtime pointer change. | Storage fault injector + independent raw bytes/OS exits/receiver effects; relevant R02–R13 slice and final R14; Mac I02 for runtime pointer. No omnibus synthetic pass. |
| B18 evidence transport/closure | Real Rust binary workflow/resource adapter, accepted Python/Rust differential cells and complete actual artifact/runtime identity. | Caller-pinned executable digest, external driver/oracles and repeatable immutable inputs; R14/I01. Shared codec/engine helper cannot be sole outcome oracle. |

## R02-specific proof plan

No test runs or spikes were authorized in R01. On R02 assignment, separate:

- Internal store tests: explicit fresh creation; incompatible/future schema refusal;
  unique claims/CAS; rollback; connection pragma verification; bounded admission;
  concurrent writer/read/checkpoint pressure; disk-full/I/O at commit; restart-safe
  migration and consistent backup/restore. These use no real backend.
- Public contract checks: complete fixture manifest verified independently; every
  envelope/exit/capability predicate; unknown optional fields; historical event
  writer version; lexical root; no state/config/backend side effect from version-info.
- Read behavior: authority-before-prefix, pending/unknown IDs, partial preparation,
  all accepted filters, opaque cursor/query/generation binding, rows/bytes/capture
  bounds and truthful final/drain/history distinctions. A populated synthetic store
  is not execution evidence. Do not import Python history to populate it.
- Failure controls: corrupt cursor and envelope; unauthorized candidate that would
  otherwise match prefix; replaced store; interrupted read; missing/corrupt captures;
  SQLite failure never maps to empty successful history. Changes to public semantics
  stop the adapter until canonical producer approval.

Read-only calls must not initialize stores or acquire execution ownership. Explicit
fixture creation is internal, not a new public operation. B10/B11/B17 remain required
before the affected public read guarantee is accepted. Unrelated B06/B07/B09/B12
proof does not prevent bounded internal R02 development.

## Measurement and evidence protocol

Retain S03-ratified engine budgets on recorded owner Apple Silicon, release builds,
provider latency excluded: cold version-info/status p95 <50ms; list/event page p95
<100ms; launch acceptance p95 <250ms excluding harness startup; write transaction
p99 <20ms with no caller-visible SQLITE_BUSY; idle task-owner RSS <30MB. Report
fresh handshake+operation end-to-end overhead separately; do not cache probes.

Datasets: CI 300 tasks/30k events/5 workflows; representative 5k tasks/500k events/
50 workflows/500 runs; stress 50k tasks/5m events/20 fake owners. Pin workload,
query distributions, warm/cold state, hardware/OS/toolchain, sample counts and
failure rates before collecting results. Include checkpoint/WAL growth and reader
latency under contention. An overload refusal is a recorded failed request, not
excluded from latency/budget accounting. Never tune thresholds after results.

Every run records source/artifact/fixture/framework/runtime/OS identities, invocation
ID, actual commands, external process/effect/capture observations and cleanup.
Uncertain cleanup fails evidence publication. Source, synthetic, packaged, stock
backend, real-provider and platform classes remain distinct. S09's 397/15/9/484
counts are historical recorded results, not rerun here. Its incomplete runtime
closure and cooperative same-UID observer remain limits. Full S09 ledger controls
historical detail; this matrix does not narrow its original E01–E22/fault families.

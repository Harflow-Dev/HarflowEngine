# E2-B10 exit evidence

Documentation/tooling checkpoint, 2026-11-17. S07 execution and the scoped
independent final review passed. The previously missing Q1 residual pin now passes its isolated system regression.
Final post-addition static and identity checks passed. This delivers the bounded
B10 exit evidence; aggregate E2/R14 and owner acceptance remain separate. Implementation source is frozen at `9de3f5a60c1c2ea606fb0701710ece6a7de397d9`.
This slice adds only the Q1 system regression and its controlled-child support;
it changes no runtime, schemas, fixtures, dependencies or capabilities.
HarflowMCP remains the canonical contract owner. No R14, public execution, client
integration, push or release acceptance follows.

## Source-equivalent retained gate

The independently recorded B8 receipt and workspace evidence under
`.claude-work/codex-2026-11-17/b8-final-gate/` cover this unchanged implementation:
workspace 2,205 passed / zero failed / 65 ignored; Python 951 passed with two skips
and three expected failures; conformance profiles 56/0, 55/1 and 50/6 with pinned
known failure identities; strict replay 926 verified / zero unverified additions
across 59 worlds. These are retained B8 results, not new B10 executions.

Release binary SHA-256 identities:

- `harflow-ctl`: `66b24818cdd7bd0f74d35bfbe751f49074457b845d0f3f6d1d0d4b4f8a55edb0`
- `hf-fixture-seed`: `010a3257696473ef4315bf84b2860a412a8b3c559599ee2261fde38f7309c8b4`

External `engine/e2/tools/verify-stage.sh b10 FRESH_ABSOLUTE_OUTPUT` delegates to
`verify-b10.py` under the workspace `.claude-work/` directory. It builds nothing,
requires unchanged runtime source and exact B8 binaries, pins the two added Q1
test files plus the harness tree and manifest,
rejects existing output, runs `python -m conformance`, requires successful process
and report status, validates the pinned runner's report/ledgers/capture hashes,
and rechecks binary/source identity and absent root. It preserves all old evidence.
`python -m conformance.runner` is not an execution gate: that module has no main
entry invocation and can exit zero without producing a report.

## Observed B10 verification

The primary executed the real pinned S07 entry point and then independently ran
`verify-b10.py`. The latter validated report invocation
`f117f69ace124f668082ccae265ff311`, all six fake modes, all three binary-smoke
operations, unchanged implementation/harness/binary identities and missing-root
noncreation. Its evidence is in workspace
`.claude-work/codex-2026-11-17/next-wave/b10-verified-final/`.
The report SHA-256 is
`2899adccf69b663afd46e54d0bb6f43b51d999283a2d93b85136c0520702714c`.
A repeated invocation targeting existing output refused with exit 1 and preserved
the original verified receipt. Scoped independent source review was clear in three rounds, including the added
system test and final exact-hash admission.
Final formatting, architecture, legacy and strict workspace/all-target Clippy checks
passed after that addition. The final helper pins both test-file hashes as well as
the unchanged executable inputs; all pre-existing implementation and test inputs
remain at the B8 source checkpoint.
Documentation whitespace and affected legacy/private-reference guards passed.
These are new B10 checks. The retained B8 full suite covers unchanged existing
implementation/tests; the added Q1 target was compiled and executed separately.
No new full-workspace or Python-suite execution is claimed for B10.

## Recovery evidence mapping

S07 only checks immutable binary handshakes and missing-root inspection, with
isolated fake-process controls. It never invokes recovery. E22/B16 recovery evidence
comes from these existing end-to-end tests in the retained full gate:

| Obligation | Exact test / evidence |
| --- | --- |
| E22 operation-bound confirmation and unchanged state on mismatch | `tests/test_ctl_state_recover.py::StateRecoverTests::test_confirmation_mismatch_never_mutates` and `test_changed_inventory_refuses_and_preserves_bytes` |
| E22 explicit archive/reset and B16 initialized fresh-store recovery | `tests/test_ctl_state_recover.py::StateRecoverTests::test_archives_bytes_and_returns_contract_shape_then_fresh_store` |
| E22 interruption/resume and receipts | `crates/hf-host/tests/state_recover_flow/restart.rs::subprocess_abort_r3a_r5_r7_reacquires_session_without_sqlite_evidence`; `completion_restart.rs::sigabrt_at_every_staged_publish_prefix_resumes_without_sqlite`; `completion.rs::partial_and_conflicting_receipts_reconcile_or_refuse` |
| B16 WAL-backed fresh store, response-loss repeat and lease-free receipt | `crates/hf-host/tests/state_recover_flow/completion.rs::wal_frames_survive_rename_and_shm_removal_and_receipt_is_lease_free` |

## Accepted limits and remaining evidence

Q1(b′) permits consistent earlier owner rows under the exclusive maintenance lock
proof, including positively detected loss. Unreadable/unqualifiable rows refuse.
A live orphan whose acknowledged claim exists only in lost commits can retain
writable archive descriptors; moving its files can break path access and an archived
reservation no longer prevents duplicate launch. This is an accepted relaxation,
not proof of absence of all owners. The named regression was missing and was
added as `crates/hf-host/tests/q1_lost_claim.rs::q1_lost_acknowledged_claim_with_live_orphan`.
Its primary run passed (one test, 0.35 seconds): actual durable Granted/readback,
known-live-owner refusal, simulated DB/WAL rollback retaining the newer watermark,
CommitLost/EarlierSnapshot admission, full archive/reset while the child is alive,
and a post-archive descriptor write changing only the archived inode. The fresh
root's bytes and inode inventory remain unchanged; cleanup follows positive child
reap. Failed fixtures are retained. This is a controlled media-loss simulation,
not real media or power-loss qualification. Independent second review was clear.

Q2(a) selects archive-only recovery; B9 in-place lost-WAL acceptance, watermark
rollback/recreation and fence clearing remain E8. The unseeded task-catalog evidence
basis is unavailable: current fixture-defined launch states cannot qualify future
production rows. E2-D must preserve claim-before-spawn, extend owner evidence to
production rows, adopt B8 refusal handling, trigger cadence and check long-lived
Store reads. The watch seam remains E2-E; payload corruption remains E8.
Failed refusal acknowledgement transport retains Unknown and does not inherit the
validated-refusal writer lifetime. These gates remain unaccepted by B10.

## Change footprint

HarflowEngine: the two Q1 system-test files above, `docs/storage-protocol.md`, `docs/store-read-path.md`,
`docs/recovery-observation.md`, and this receipt. HarflowMCP: only the Rust recovery
qualification paragraphs in `docs/contracts.md` at local commit `0bfa115`. External workspace tooling:
`engine/e2/tools/verify-stage.sh` and `engine/e2/tools/verify-b10.py` beneath
`.claude-work/`. Existing MCP `dist-rc/` is preserved.

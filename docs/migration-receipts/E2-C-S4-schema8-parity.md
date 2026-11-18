# E2-C S4 schema 8 parity (provisional, substitute control)

Evidence checkpoint, 2026-11-18. **Provisional**: the new pair is built from `06c4c20`, before S5/S1/S2 land; S4
reruns on the S3 freeze pair (e2c-next-plan S3.3). **Substitute control**: the original schema 7 control was deleted
on 2026-11-18; owner decision (b) rebuilt it from source. Acceptance remains the owner's.

## Plans and tools

- Parity plan: `.claude-work/codex-2026-11-17/usable-mac-first-task/schema8-parity-plan.md`, amended by
  `parity/s4-substitute-control-plan.md` revision 5 (reviewed by Opus and Astra over 3 rounds; revision 5 adds the
  replay scratch-path pin, coordinator-approved under the owner's delegation).
- Tools: `parity/s4-tools-plan.md` revision 3; tools under `parity/tools/` (unit suite 150 OK, 1 skip pinned to
  `55826c5`). Code review: 3 rounds plus an owner-delegated confirm-only pass, then a confirm-only review of two
  post-run fixes (live-identity P1 exception for `task_processes` leaves; strict ctl envelope parsing). Ledger in
  `parity/s4-tools-plan.md`.

## Control pairs

Built from `git archive` with a verified tree, offline, toolchain 1.99.1, sanitized environment, original command
`cargo build --release --locked --offline -p hf-host --bin harflow-ctl -p hf-fixture-seed --bin hf-fixture-seed`
(receipts: `.claude-work/engine/e2/s4-controls/{old-9de3f5a,new-06c4c20}/build-receipt.json`).

| Role | Source | `harflow-ctl` | `hf-fixture-seed` |
| --- | --- | --- | --- |
| old, substitute, schema 7 | `9de3f5a` | `3ab62a2c546e7cf5d85bd7dfd3ad1ada0dc52c879676166e6bb2b0b2d2295df6` | `87cf5069093921ee221802d93a38a84da6e2492c436c5cc4578e04292d37bf2b` |
| new, provisional, schema 8 | `06c4c20` | `e3225b27e826e74508a2de217f40997def28b04b102a158eecce3e9ed0815419` | `a97b845de0b3ca725ec8be8d8cb3bb53cc84d77fd95555159244aae7838bbdb0` |

The substitute replaces the deleted pair `66b24818…edb0` / `010a3257…c8b4`; no byte identity is claimed. Provenance
record `parity/control-provenance-provisional.json`, SHA-256
`d2a45ed1b64f58a6ba1a47682529dd6fd1bcbad936b9067ee3849ed326706c05` (read-only).

## Qualification of the substitute

Every frozen replay ran with `TMPDIR=/Users/Shared/Code/Harflow/.claude-work/engine/e2/s4-tmp` (resolved path
exactly 56 bytes). Artifact `encoded_len` embeds the absolute target path under the replay scratch directory, so a
frozen replay is valid evidence only with a 56-byte resolved `TMPDIR`. The first Q0, run under `/private/tmp`, showed
28 `encoded_len` differences of 44 bytes; a diagnostic with the original-length path gave `SAME`; the amendment was
revised and Q0 rerun.

- Q-noise (two replays, no allowances): `SAME`.
- Q0 against the deleted control's own retained replay output, no allowances, `--strict-sizes`: 59/59, 0 failures,
  0 physical differences, `SAME`.
- Q1 against the frozen baseline with the historical allowances: 0 failures, 892 differences explained by
  `commit_sequence`, 0 unexplained, 926 verified, 0 unverified, `SAME`; replay 59 worlds, 494 checkpoints, 0 rc
  failures, 225 stdout mismatches, 0 non-digest.
- Q2 conformance-full with the substitute pair: 56 pass, 0 fail.

## Schema 8 parity (run r2)

- Old and new snapshot replays: 59 worlds, 494 checkpoints, 0 rc failures each. 1A qualification passed.
- Physical predicate: `physical_acceptance` true; 446 checkpoints checked, 48 not applicable, 0 failures, model
  validated. The live-identity exception was not exercised in r2 (no width difference occurred); it is covered by unit
  tests only. In the earlier r1 run six `task_processes` leaves differed by one byte because a live `start_micros`
  value needed a 3-byte instead of a 2-byte SQLite integer; that is per-run variation.
- 1B (new vs old, no allowances, substituted sizes) and 1C (new vs frozen baseline, historical allowances): passed;
  0 failures, 0 unverified.
- Refusal gate, both directions refused with the predicted predicate and unchanged inventories:
  - new ctl on a schema 7 root: `incompatible` / `unsupported_layout`; cross-version `add_task`:
    `storage error: Incompatible`.
  - old ctl on a schema 8 root: `incompatible` / `newer_store`; cross-version `add_task`:
    `storage error: FutureSchema`.

Report SHA-256 (under `.claude-work/engine/e2/s4-controls/runs/`): old-r2 manifest `09e53fe1…ef98`, new-r2 manifest
`48afdee2…8e8d`, 1A `fd8df42b…9dac`, physical `9f131f52…2833`, 1B/1C `a46a50fc…d719`, refusal `046d46b9…63b2`. Full
commands, exit codes and inventories: `s4-controls/S4-results.md`.

## Frozen corpus

`.claude-work/engine/e2/seed-baseline/` was inventoried (lstat, hashes, directory mtimes) around every Python
invocation; no difference was observed and no bytecode was written.

## Not accepted here

Final parity on the S3 freeze pair; any claim that the substitute is byte-identical to the deleted control.

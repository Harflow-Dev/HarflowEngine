# E2-B4 prerequisite: recovery proposal lock binding

Delivered locally on 2026-11-16. This corrects proposal identity binding before
the recovery flow is implemented; it does not deliver B4 recovery or a ctl mutation.

Inspection previously recorded the optional legacy lock's inode in `lock_identity`,
while `MaintenanceSession` compares that field with the Rust `store.lock` inode.
With both files present this refused an unchanged root; with only `store.lock`
the null binding could admit a later replacement. Inventory now records
`store.lock`, as the maintenance protocol requires. The optional legacy exclusion
lock and inventory exclusions are unchanged. An originally absent Rust lock keeps
the accepted null-binding normalization for first creation and retry.

## Evidence

- Before the fix, the new integration tests recorded four failures and one pass:
  wrong inode with both locks, absent Rust-lock binding, a legacy-only binding and
  admission after Rust-lock replacement. The null-normalization case passed.
- Final scoped runs: five host integration tests, 17 inventory tests and 13 Python
  proposal-token tests passed. The replacement test retains an open handle to
  the original inode and requires a distinct replacement identity.
- Final full Rust workspace gate: 2,089 passed, zero failed, 63 ignored; 302 summaries.
- Full Python suite: 937 tests, OK with two skips and three existing expected failures.
- Formatting, architecture and legacy-name guards and all-target clippy with
  warnings denied passed. Two independent review rounds closed without findings.

Kernel/FIFO fixtures ran outside the sandbox because the sandbox denies those
observations and fixture creation. All roots were temporary owned fixtures. No
live state, provider, desktop interaction, push or release was used.

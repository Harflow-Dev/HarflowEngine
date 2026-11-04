# Internal fixture storage foundation

R02-A/R02-A2 implement no public engine operation. The host remains the unsupported
scaffold; no client may select this as a compatible engine. See the
[R02-A receipt](migration-receipts/R02-A.md),
[R02-A2 receipt](migration-receipts/R02-A2.md),
[current cooperative protocol](storage-protocol.md) and
[R01 design](decisions/R01-architecture.md).

`hf-storage` exposes explicit fixture initialization, identity probing/opening,
typed insert/put/delete/read batches, and internal checkpoint/recovery primitives.
Initialization requires a nonexistent root; existing empty, foreign and Python roots
are not adopted. Generation/schema are persisted in SQLite; a bounded immutable
protocol marker identifies the initialized root. R02-A's old lock-only fixtures
refuse rather than silently upgrade. No public creation, recovery, task/event,
import or capture API exists.

Independent handles hold shared lifecycle ownership; write/open/snapshot/checkpoint
operations use a separate cooperative I/O gate. Per-handle mutex admission rejects
overlap without queuing late work. Read batches use SQLite snapshot isolation.
Explicit recovery/TRUNCATE requires exclusive lifecycle ownership; PASSIVE checkpoint
may return incomplete progress while readers remain. Checkpoint frame counts,
not busy status alone, determine completeness. There is no schema migration API.

Probe never opens source SQLite. Under the gates, it qualifies a private bounded
main+WAL copy using SQLite, without copying SHM. A missing root creates nothing;
foreign/unknown/future/corrupt identity refuses before original SQLite access.
Probe may write private scratch, not source contents/sidecars. Normal open is
explicitly authorized internally to let SQLite recover a legitimate residual WAL
and rebuild SHM only after private-copy qualification. There is no parser, immutable
live-WAL shortcut, file deletion/reset or automatic external-effect replay.

Source connections use WAL/FULL/foreign keys and suppress automatic and last-close
checkpoints. Explicit checkpoints honor the gate. Snapshot size/deadline bounds
fail closed; they are qualification limits, not performance results. Scratch is
private and cleaned on normal outcomes; supervising tests clean exact invocation
remnants after killed children, since SIGKILL bypasses RAII.

Transactions accept typed bounded mutations, never arbitrary callbacks/SQL.
Absolute deadlines govern admission and remaining SQLite busy waits. OS I/O and
individual SQLite calls cannot be forcibly preempted; completed commit is reported
truthfully, and uncertain commit/binding results remain explicit. No public retry
semantics follow from internal errors.

Local same-host cooperative Unix roots only. Metadata checks reject symlinks and
observed root/database/lock/marker replacement, not atomic hostile path races.
Callers supply canonical temporary parents; raw writers bypassing protocol gates
are not supported schema changers. Non-Unix binding refuses. No Linux/Windows,
network-filesystem, arbitrary corrupt-WAL detection, directory-fsync/power-loss,
performance or R14 qualification follows from targeted macOS process tests.
SQLite decides valid committed WAL history; ignored/torn/uncommitted tails do not
become proved external effects or permission to replay.

SQLite dependency remains exact rusqlite0.41.1, default features disabled, bundled only;
libsqlite3-sys0.39.1 supplies SQLite3.54.1. Cargo.lock pins the closure. The guard
keeps exact storage edge/version/features and all other layer/domain/unsafe rules.
Safe rusqlite configuration is used; no workspace unsafe or added runtime framework.
Use pinned Rust1.99.1/rustfmt/clippy and documented checks. No shell profile change
is required; tests use isolated temporary fixture roots and owned processes only.

R03-B1 deliberately initializes fresh schema2 roots and rejects older schema1 roots
without an automatic upgrade. It adds a separate internal ownership namespace and
exact storage-only SHA-256 verification dependency; see the
[ownership foundation](ownership-foundation.md). This does not make fixture task/
event DML authoritative execution state or add a supported engine operation.

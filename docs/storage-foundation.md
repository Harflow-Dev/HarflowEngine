# Internal fixture storage foundation

R02-A implements no public engine operation. The host remains the unsupported
scaffold; no client may select this as a compatible engine. See the
[R02-A receipt](migration-receipts/R02-A.md) and [R01 design](decisions/R01-architecture.md).

`hf-storage` exposes explicit `Store::initialize_fixture`, `Store::open`, `probe`,
identity and typed insert/put/delete/read batches for isolated fixtures. Initialization
requires a nonexistent root and atomically claims it with directory creation;
existing empty, foreign and Python roots are not adopted. Generation/schema are
persisted in SQLite. Missing-root probes create nothing. There is no public creation,
recovery, task/event ingestion, import or capture API.

One Store owns an exclusive OS lock on the persistent `store.lock` for its lifetime.
Callers may share that handle; mutex admission rejects overlap with `Busy`, without
queueing later effects. Another open or active-store probe refuses. A quiescent
probe takes a shared existing lock and does not create the lock file. Locks are
cooperative: external SQLite writers that bypass this protocol are outside this
fixture API's guarantees. Process death releases the OS lock, not execution or
reservation uncertainty in future engine layers.

Initial open/probe refuse a nonempty residual WAL with `NeedsCheckpoint` before
SQLite access; they never automatically checkpoint/recover another store. No raw
WAL parser or SQLite immutable mode is used. Live multiprocess reopen and recovery
need a separately reviewed qualification protocol before detached-owner work can
rely on this store. This limitation does not remove R01's eventual concurrent-owner
requirements. Normal transactions on an already opened handle use WAL.

Every writable connection enables and verifies WAL, FULL synchronization and foreign
keys. Transactions accept only typed bounded mutations, not arbitrary callbacks or
SQL. One absolute deadline governs admission and recomputed SQLite busy waits;
reads enforce input/output row/byte limits. There is no background queue. OS I/O
cannot be preempted at a deadline; a completed commit is reported truthfully, and
commit/post-commit binding failures report `CommitOutcomeUnknown`. No public retry
semantics follow from these internal error variants.

Root/database/lock and sidecar checks reject symlinks and observed inode replacement
on supported Unix filesystems. Ancestor symlinks are rejected, so fixture callers
must supply a canonical temporary parent (macOS `/tmp` and `/var` aliases need
canonicalization). Checks are not atomic hostile-path nofollow protection. The
non-Unix binding adapter refuses: Windows scaffold compilation is not storage
qualification. No directory/device power-loss durability or performance acceptance
is claimed by successful rollback/reopen tests.

Dependency: exact rusqlite 0.41.1 with default features disabled and bundled only;
libsqlite3-sys 0.39.1 supplies SQLite 3.54.1. Cargo.lock pins the transitive closure.
Reviewed roles are SQLite wrapper/native SQLite and its C build tooling, not new
domain effects. The architecture guard permits only this storage dependency with
its exact version/features, retains all other edges and domain purity rules, and
rejects non-normal, renamed, optional or target-specific substitutes. Dependency
internal native/unsafe code remains distinct from forbidden workspace unsafe.

Use the repository's pinned Rust 1.99.1 with rustfmt/clippy and documented development
checks. Local setup may require `/Users/nayanda/.cargo/bin` on PATH for this account;
no shell profile change is required. Tests use only isolated temporary fixture roots.

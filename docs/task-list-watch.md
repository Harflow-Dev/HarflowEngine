# Internal task-list invalidation foundation (R02-W)

This is an internal Rust store/wait seam plus consumer mapping, not a supported ctl
command, production task catalog or caller verifier. The default host remains
unsupported and advertises no capability. HarflowMCP remains the canonical contract
owner until accepted O04. See the [receipt](migration-receipts/R02-W.md).

## Exact adopted contract

Corrected HarflowMCP0.0.10 source
`8a936de61b5f9f2fdff671b65b5384259267350b` supplies the selected
[task-list-watch.v1 spec](https://github.com/Harflow-Dev/HarflowMCP/blob/8a936de61b5f9f2fdff671b65b5384259267350b/docs/contracts.md#task-list-invalidation-wait-task-list-watchv1).
The entire207-fixture bundle is vendored, manifest
`ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906`.
[Consumer provenance](../tests/fixtures/contracts-v1.provenance.json) records every
old/new hash and classification. Prior199-fixture receipts remain historical.

The canonical operation is one ctlv1 invalidation wait, not a daemon, transcript or
MCP tool. Wait is0..30seconds (default30 in a future host; ignored without cursor),
cursor at most2048 UTF8 bytes and whole success document at most16384bytes.
States are baseline/changed/timeout/reset/unavailable. Every non-unavailable result
has a nonempty next cursor; unavailable has null cursor. Reset/unavailable require
a bounded reason; the other states omit reason. Unknown additive fields survive
consumer parsing without becoming facts.

Changed signals an admitted publication, including conservative false positives.
Timeout means no producer invalidation was observed under established continuity;
it does not establish unchanged headers, owner liveness, execution or terminal facts.
Consumers must still reconcile public lifecycle deadlines. Malformed/wrong-scope/
wrong-root cursors refuse as task_list_watch_cursor_invalid. A different epoch resets
with epoch_changed; a future revision resets with discontinuity. Cursors are hints,
not authentication, authorization or evidence of task state.

## Atomic Rust publication boundary

Fresh schema3 initializes one task_watch_meta row (singleton1, version1, revision0)
in the exclusive root initialization transaction. Root generation is its epoch.
Schema1/2/future roots refuse, with no automatic upgrade, reader initialization,
reset or repair. The selected fresh schema requires canonical lowercase32hex store
generation in marker/database, exact UTF8 byte length checked before allocation.
Embedded-NUL suffixes and noncanonical source generations refuse without rewrite.

Admitted task metadata and watcher revision share the SAME short SQLite IMMEDIATE
transaction. A task_headers replacement advances revision before commit, under the
existing cooperative I/O gate. Abort or precommit process death publishes neither;
postcommit response loss retains both. Revision exhaustion refuses and rolls back
rather than wrapping or silently changing epoch. WAL/FULL/foreign keys, manual
checkpoint policy and64MiB main+WAL qualification remain unchanged.

No persistent open-PID fence or begin/end token API exists. The exact lexical SQLite
transaction owns publication; observers cannot finish it. A live uncommitted writer
exposes no changed facts and no revision, so timeout before its eventual commit is
safe. A later wait from the retained cursor observes the commit. Python needs
pre/post file fences because metadata replacement and catalog transactions are not
one atomic write; those private mechanics are not copied into Rust.

| Current path | Participation |
| --- | --- |
| replace_task_fixture | Republishes task_headers and advances revision atomically, even for identical bytes. Pending values in that whole replacement do not make it a pending-only operation. |
| replace_pending_fixture | Writes only task_pending and advances the paging dataset generation; no task metadata invalidation. Duplicate/header-colliding IDs refuse atomically. |
| Task/status/list reads and watch observations | Read only; no revision/lease/fence writes or observation-side publication completion. |
| Stored event fixture DML | Separate namespace; no task metadata publication. |
| B1/B2 ownership/claim/cancel/release and generic fixture_records | Separate namespace; no task metadata publication. |
| Future production lifecycle/catalog writers | Not implemented. Must join the same atomic metadata/revision commit before watch coverage can be claimed. |

Current task header/status/observation fields are stored fixture metadata. They are
not dynamic OS liveness overlays. Pending launches, continuation reservations and
read-side liveness reconciliation remain excluded. Rust borrows no legacy-writer
exception: every currently admitted task metadata writer participates. Uncooperative
raw database writers remain unsupported; this is not broad R04 corruption coverage.

## Authority, qualification and waiting

TaskWatcher is constructor-injected. It prepares authority before any repository
work and admits only explicit Global direct-fixture authority. Preparing, blocked
or managed Closed scopes refuse before root access or cursor decoding. This grant
is not real OS caller/lineage verification. WatchSession is a trusted internal
repository port, not an independently authorized public operation.

Qualification precedence is authority, wait validation, root qualification, cursor
decoding, source observation. Missing root returns unavailable/state_missing even
with a malformed cursor and creates nothing. A present unmarked/foreign root or
missing ownership infrastructure refuses as incompatible; ownership contention is
StateBusy. Watch-source malformed/unavailable observation returns unavailable/
observation_unavailable. Cancellation and caller deadline are distinct errors and
never successful empty timeouts.

Each operation opens one qualified Store handle. Setup uses at most two seconds of
admission budget, bounded by the caller deadline. Wait ceiling starts after root
qualification and decoding but remains within the overall wait+two-second/caller
budget. Every source observation receives at most50ms SQLite admission/busy budget,
not the full30second wait. Cancellation/deadline checks follow blocking work and
cursor encoding, before returning a result.

Source reads project at most two rows of integer scalars, with type checks before
allocation. They do not scan task headers or recursively snapshot catalog JSON.
Routine root/identity qualification and source binding checks still occur; the
initial Store qualification may copy up to the existing64MiB limit. No memory/RSS,
scan throughput, p99 or scale qualification is inferred from these structural bounds.

Readers drop the Store mutex/transaction before waiting. RealWatchControl has a
cloneable shared cancellation flag and Condvar. Spurious wakes remain inside its
absolute-target loop; within one waiting operation, source polling is at most once per50ms, with no background
thread or queued late work. Injected controls must honor the same wait-until
contract. Root replacement during a retained wait yields unavailable rather than
timeout; a fresh qualified operation at the same path resets the old epoch. A truly
different selected root refuses the foreign cursor.

SQLite, filesystem and kernel calls are synchronous/nonpreemptible. Admission
budgets do not claim hard wall-clock preemption of arbitrary kernel/filesystem
stalls. Contention, cancellation and uncertainty never become unchanged facts.

## Consumer mapping and busy refresh

WatchJsonCodec uses bounded URL-safe-base64 JSON carrying v1, fixed scope, root
hash, epoch and signed-range nonnegative revision. The root digest reveals no path.
Known fields and scalar types are strict; bool is not integer. The representation
is opaque producer detail, not a public credential format or Python persistence.

WatchPermit qualifies one version-info data document and is consumed once for one
watch response. It validates stable product line, ctl/version-info formats and
watch capability, selected root and configured platform. Unknown additive
capabilities are tolerated. This pure data qualifier does not perform an actual
fresh OS invocation, bind HOME/environment/cwd/executable identity or authenticate
a runtime. Future host/client integration must supply that fresh per-operation
probe and qualify public authority. The16KiB handshake parser ceiling is a local
bounded parser limit, not a new canonical version-info limit.

ConsumerState permits no first snapshot until baseline (or a fresh reset boundary)
has been retained. It keeps one wait in flight, stores each next cursor BEFORE
queueing refresh, and coalesces invalidations received while refresh is busy. Timeout
queues lifecycle reconciliation AND snapshot refresh; a later invalidation cannot
erase that work. Unavailable/failed waits require rebaselining and reconciliation.
Actual public snapshot/lifecycle operations remain caller responsibilities.

The result encoder takes an explicit stable product version. Tests can use the
canonical Python fixture version or actual experimental Rust0.0.1; the encoder does
not relabel the Rust runtime0.0.10 or advertise support.

## Dependency and retained gates

Only hf-interfaces adds exact base64=0.22.1, default features disabled, alloc only,
MIT/Apache-2.0 with no normal transitive dependencies. Archive SHA256
`72b3254f16251a8381aa12e40e3c4d2f0199f8c6508fbecb9d91f575e0fbb8c6`
is matched to the registry and Cargo.lock. Existing storage-only sha2 computes the
root key; domain remains serialization/effect free. No new native/async runtime or
global tool is installed.

Actual supported watch CLI, fresh caller/runtime qualification, production task
catalog writers and client adapters remain gates before capability advertising.
B2c capture durability/lifecycle, R04 faults, B10/B11/B17 and R14 acceptance remain
outstanding. Corrected Python's owner-accepted mixed-legacy-writer exclusion applies
only to that temporary producer; it is not permission to weaken Rust continuity.

## Subsequent B2c schema

Current fresh roots use schema4 for the separate [private capture/completion
foundation](capture-completion.md). The schema3 watch foundation above is its
historical entry stage; schema3 roots now refuse without upgrade or repair.
Watcher publication and its current writer inventory are otherwise unchanged.

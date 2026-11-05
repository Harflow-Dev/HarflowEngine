# Private owner output drainage

This internal fixed-helper design extends [retained owner sessions](migration-receipts/R04-D.md).
HarflowMCP remains the canonical contract owner. No public runtime, caller authority,
backend, recovery control or compatible engine is added.

## Ownership and ordering

The original owner main thread retains its Store/root lease, raw output files,
original child, input and sole waiter. After the exact EffectDone framed prefix,
one private reader receives the two output FDs by move, before acceptance or capture
persistence can stall. Registered child identity and the SpawnObserved fence bind
that reader; acceptance must preserve operation, generation, attempt, owner and
child. Neither stored identity nor a reader snapshot grants execution authority.

The reader performs no storage I/O. A finite data-only FIFO preserves a contiguous
prefix of each stream. On its first full/disconnected send it discards that batch
and all later bytes, continuing to drain and count. It never resumes enqueueing
after a gap. Discarding capture and stopping reads are separate controls. Read-only
views return owned observations without control rights; final EOF/error/count facts
bypass the FIFO and become immutable.

While healthy, the main thread preserves raw append, sync, qualified index/event
commit, then acknowledgment. Ambiguous writes permanently poison the session; no
later storage mutation, retry, repair or truncation follows. Reader-discarded bytes,
accepted-but-abandoned bytes, ambiguous raw tails and unknown unread tails remain
separate. Enqueued bytes are not automatically durable bytes.

Terminal publication requires joined immutable reader completion, reconciliation
of every accepted batch, observed EOF on both streams, no capture loss/error/stop,
original-child exit and reconciled input. Exit alone and EOF alone are insufficient.
Volatile successful exit/drain can coexist with durable Unobserved exit after poison.
Reservations persist; no release, replacement, replay or adoption follows.

## Finite fixture bounds

| Resource | Bound |
| --- | --- |
| Operations | One per fresh owner process; no process reuse or scheduler queue |
| Owner application threads | Main plus one reader; no background storage writer |
| Output descriptors | Two moved, no duplicated pipe or new sentinel authorities |
| FIFO | Four data batches, each at most16384 bytes per stream |
| Logical payload in flight |131072 queued +32768 producer +32768 consumer =196608 bytes |
| Extra disk spool | None |
| Read work |8MiB per stream,4096 poll rounds,8192 read attempts |
| Poll round | At most two reads, one per stream; at most20ms poll |
| Reader lifetime | Earlier of original owner deadline or start+8s |
| Existing private budgets | Dispatcher10s, owner8s, effect4s; no observer extension |
| Raw/index bounds |65536 bytes per stream,16384-byte append chunks,128 events |
| Coordinator |256 permanent sentinel/quarantine FDs,1024 shared holders; no eviction |
| Settlement | Stop and bounded finished checks; join only a finished reader |

Payload accounting excludes allocator metadata and thread stacks; this is no RSS
measurement. Reaching a read/work/deadline limit reports incomplete capture with
unknown unread tail, never synthetic EOF. Closing a pipe on shutdown can affect the
producer; this is not product cancellation or successful drainage.

## Failure boundary

Private owner cleanup must settle both reader and original child before Store/root
teardown, including error and unwind paths. An unresolved reader or child takes the
explicit private fail-closed process path rather than detaching a worker, unboundedly
joining, or releasing the Store first. This policy is not a general service policy.

If a main-thread storage syscall never returns, owner shutdown is not bounded.
The main thread retains Store/root ownership while blocked. Finite reader limits
can stop further reads, but cannot return control to that main thread or fabricate
an acknowledgment. OS calls remain nonpreemptible.

A process-isolated storage alternative requires a separate reviewed IPC, commit,
identity and lease handoff design. Even process termination cannot guarantee bounded
completion under uninterruptible kernel I/O. Unresolved writers/commits retain
uncertainty and reservations. No such storage process, automatic restart or central
service substitution is implemented by this reader design.

## Remaining qualification

Finite injected storage gates establish progress independence, not actual failed
sync, ENOSPC, device or power behavior. Actual storage fault setup needs a separately
authorized disposable environment. Fixture budgets are not production admission,
retention, idle or backend budgets. Real caller/enrollment/grant/outcome semantics,
containment and public integration remain gated; the three R04-B policy proposals
remain unaccepted. Supported internal D3 mechanics may proceed under the stage
amendment without turning each suffix into a user handoff.

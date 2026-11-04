# Internal task reads, R02-B

See [receipt](migration-receipts/R02-B.md) and [storage protocol](storage-protocol.md).
The scaffold still reports unsupported engine operations. No public command,
capability, cursor encoding, task lifecycle or production caller verifier is added.

## Two bounded stages

B1 supplies pure domain query/identifier/coverage values, an application service with
injected authority/read ports, and indexed fixture reads in storage. Opaque stored
header/status bytes are outside the domain. B2 supplies independent consumer views
of existing ctl task/page/error documents and the full pinned fixture bundle.
Neither serializes the internal query result into a supposedly compatible command.

`TaskReader` obtains authority before repository access. Preparing/blocked authority
returns typed unready/refusal without counts, candidate enumeration or repository
calls. Authorized global/closed scopes come only from the injected trusted port;
there is no production implementation proving OS/direct/worker/builder/orchestrator
identity. A request cannot choose a direct-caller flag. Closed candidate queries
never consult global candidates; missing and unauthorized IDs expose no global
matches. This is fixture scope enforcement, not completed B10 authority qualification.
Missing/false owner observations never upgrade the authority grant.

New fixture initialization creates empty task-read tables under the existing
exclusive initialization transaction. Their dataset has its own version/generation.
Explicit `replace_task_fixture` performs bounded DML replacement and generation
rotation under the writer gate, not execution or a task-write lifecycle. Old roots
without task tables and unseeded datasets return unavailable; reads and replacement
do not create/upgrade missing tables. Root schema remains the existing fixture
foundation; no migration/public creation path is introduced.

## Query and observation coverage

| Internal behavior | Pinned contract relationship and limit |
| --- | --- |
| IDs | ASCII `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`; exact wins, canonical32hex/hyphen UUID never prefix, prefix minimum8, case-sensitive literal underscore. Ambiguity at most5 candidate IDs plus truncation; full public ambiguity diagnostics/echo serialization are not implemented. |
| Scope | Namespace precedes exact/prefix lookup; closed scopes bound100 supplied IDs. Actual managed role/ownership derivation remains absent. |
| Pending | Separate fixture-supplied exact ID presence, not a task record; never prefix candidate/list row. Typed Pending supplies no launch state, reservation/owner liveness or public status body. |
| Pages | Limit1..100; `(stamp,id)` descending keyset; independent supplied active observation and session filter up to256characters. Sort stamps are explicit fixture integers, not a new public timestamp parser. |
| Explicit IDs | Separate bounded request mode, deduplicated/request order; no silently ignored history filter/cursor. Missing IDs are lookup outcomes and omitted rows. Public adapter error/drop/echo mapping remains unimplemented. |
| Stored status | Same scoped resolver, optional stored status bytes and independent recorded-status/active/exit/drain/owner facts. No supervision/provider replay, current process check or inferred successful delivery. |
| Evidence | Discovery/ownership/source completeness supplied explicitly. Unknown activity has separate incomplete evidence; matching-row count stays unknown when required evidence is incomplete. Count is not canonical total_active_count/root accounting. Pagination partial/more is independent of evidence completeness. |
| Bounds | Fixture header max2048bytes; status bytes also local2048byte bound, not the public full-status limit. Pages bounded128KiB with512reserve plus cursor/scope metadata accounting and Store limits. Oversized input/materialization refuses rather than falsely complete truncation. |
| Cursors | Internal typed store/dataset/query-filter/authorized-namespace binding; dataset replacement invalidates. Page-size changes preserve keyset semantics. No wire encoding/TTL/public reset/error contract is invented. |
| Snapshot | Dataset generation, resolution, rows and counts share one SQLite read transaction. Reads use an already-open qualified Store; no root init, recovery, retention or execution is triggered. |

The internal byte budget accounts for materialized opaque bytes/metadata; it is
not a serialized public ctl response guarantee. No public result serializer exists.

All ordinary runtime `status` fields require the canonical supervision/control/
provider projections. An indexed `status` string is not proof of activity, observed
exit, terminal output drain, authority or completion. Unknown/null/false and absent
stored facts stay separate. Unknown source/ownership does not become complete
history, zero cost/count or direct authority. Workflow associations/root counts,
predecessor/successor discovery and compact group/title/repo/status/backend/since
filters are outside this internal query surface, not silently implemented partially.

## Consumer-only documents

`hf-interfaces::task_consumer` retains the original JSON value and exposes typed
Missing/Null/Value accessors. It independently validates ctl product/format/integer
wire v1 and numeric release line before task/page/error interpretation; patches and
build metadata remain additive. It preserves unknown optional fields and enum/error
strings, validates selected known field types, treats cursors as opaque, and never
infers observations from missing fields. Error documents remain distinct from task
bodies; the pending-predecessor error fixture does not commission successor support.

The parser's default1MiB byte ceiling and serde_json's default recursion limit128
are local consumer resource bounds, not new public command limits. Maximum100 page
items and2048character opaque cursor reflect selected task-page constraints. The
consumer does not verify artifact authenticity or replace a fresh runtime handshake.
It does not establish that opaque fixture bytes agree with independent index facts;
a future producer/adapter must derive and validate those inputs under its accepted
contract. No production task publisher is present in R02-B.

Dependency: exact serde_json1.0.154, defaults disabled/std only, permitted solely in
hf-interfaces with exact owner/version/features/kind enforcement. MIT OR Apache2.0,
declared MSRV1.71. Actual normal/build tree adds itoa/memchr/serde_core/zmij;
Cargo.lock also records inactive resolution entries, including derive tooling, but
no domain serialization/derive dependency or guard wildcard is enabled.

## Immutable fixture adoption and separate deltas

Complete201file bundle (199fixtures plus2manifests) is vendored under
`tests/fixtures/contracts-v1`. Canonical source:
b0185be855fd13acc94cb2d2008400d285d15df2; S05 fixture producer:
b69b5754a7b6496aaf5e3106116e302ea7a10675; manifest SHA-256:
383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0.
Consumer tests/builds use repository-owned bytes, no sibling runtime or network
fixture fetch. Independent hash verification of all199 files is adoption evidence,
not199 task-read decoder or runtime executions. Selected fixtures and synthetic
negative controls exercise only declared consumers.

Mac's separate0.0.8/f304a65 manifest
6e742e1966d2b48afbfc80268b19f27da1e3f75f046f5683a35fc671228c7ce1
is not adopted here. Any later delta pins exact producer spec/capability/full bundle
and consumer evidence. Moving0.0.9 drafts are not inputs. HarflowMCP owns public
contracts until accepted O04; no Rust-only wire semantics or app MCP fallback.

## Required watcher handoff: proposed R02-W

The owner explicitly requires Rust to implement the new public watcher as migration
scope. It is not optional and must be qualified before affected read/observation
acceptance and Mac-on-Rust integration. R02-B remains independent of that unfinished
producer contract; actual R02-W scheduling and immutable producer readiness remain
prerequisites, not another request to approve watcher scope.

Authorized coordination with the Mac M03 chat confirmed public polling is selected
now (1s visible/busy/error/preparing;30s hidden idle) with a replaceable source.
Mac supplied planning receipt at e15889c and the workspace Cursor handoff. No final
watcher producer commit/capability/fixture digest was supplied. Proposed command
`task-list-watch`/capability `task-list-watch.v1`, states,0–30s/2048byte/16KiB bounds
are candidates, not ratified schemas. The handoff file's observed SHA-256 is
cc7448820327aa1fd3c53906da6429fc5bdcdd8b3c606d7e0212064964a52f5c;
it is an uncommitted workspace input, not canonical acceptance. No Cursor-owned
Python code was edited or duplicated. The owner says Cursor owns that producer.

Propose bounded R02-W1 input/adoption audit followed by assigned R02-W2 Rust
implementation/conformance, retaining the following required questions until exact
canonical decisions arrive:

- Distinguish invalidation from authoritative task/status reads. Establish baseline
  before snapshot, retain it through snapshot, and preserve next acknowledged cursor
  through refresh; never obtain the first boundary only after reading.
- Finalize continuity, gaps/reset/unavailable, malformed/wrong-scope refusal,
  response loss, root/producer replacement, overflow/eviction, change-then-revert,
  multi-file publication/crash windows and mixed old/new writer policy. Current
  fixture dataset generation is not a durable watch journal or liveness oracle.
- Retain invalidations arriving during busy refresh until consumed/coalesced follow-up;
  never advance the cursor then discard its only refresh request. Reconcile owner
  death without metadata writes on a deadline even after watcher timeout.
- Finalize direct/managed/root qualification before disclosure, wait/cursor/document/
  resource bounds, cancellation of observation only, one wait in flight, generation
  fences, bounded demand/backoff and fresh compatibility per wait and public refresh.
- Adopt exact accepted producer spec/capability/full fixture bundle and artifact
  identity, then prove Rust behavior with independent publication/process/fault
  oracles on each claimed platform. No private persistence or agent-MCP app fallback.

Current internal store/read generation and I/O gates are architectural facts, not a
speculative watcher API or proof of cross-process notification continuity. No watcher
implementation began here. B10/B11/B17, R02-C events, R03 execution, scale/copy cost,
WAL-growth/checkpoint scheduling and broader fault/R14 gates remain outstanding.

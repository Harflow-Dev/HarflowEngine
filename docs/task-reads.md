# Internal task reads, R02-B

See [receipt](migration-receipts/R02-B.md) and [storage protocol](storage-protocol.md).
R02-B added no public command, capability, cursor encoding, task lifecycle or
production caller verifier. The scaffold still reports unsupported engine operations.

Current `harflow-ctl` provides `version-info` and `state-inspect` everywhere; on
macOS arm64 it also provides `list`, `status` (including `--brief`), `status-batch`,
`task-list-page` (including successors), `task-events`, `task-prompt`,
`task-artifacts`, `recovery-activity`, `interaction-events` and `statistics-page`.
Store-backed commands classify the root before store opening and use one
`ReadSession` snapshot under a shared lease. `task-events`, `task-prompt` and
`statistics-page` check caller markers and may refuse before root classification.
Missing-root reads create nothing. The caller gate admits
`NoAgentDetected`, not a human credential, and refuses unresolved agents under
`REFUSE_UNRESOLVED_AGENTS`. This does not qualify managed ownership or lifecycle
control. Watch command/capability remains unavailable until E2. See the
[partial E1 receipt](migration-receipts/E1.md).

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
identity within this R02-B seam. The ctl adapter separately uses process/lineage
observations and the policy in `hf_domain::ctl::caller`.
A request cannot choose a direct-caller flag. Closed candidate queries
never consult global candidates; missing and unauthorized IDs expose no global
matches. This is fixture scope enforcement, not completed B10 authority qualification.
Missing/false owner observations never upgrade the authority grant.

New fixture initialization creates empty task-read tables under the existing
exclusive initialization transaction. Their dataset has its own version/generation.
Explicit `replace_task_fixture` performs bounded DML replacement and generation
rotation under the writer gate, not execution or a task-write lifecycle. Old roots
without task tables and unseeded datasets return unavailable; reads and replacement
do not create/upgrade missing tables. Current fresh roots use schema6: typed
`task_facts`, `task_processes`, `task_prompts`, `artifact_manifests` and
`artifact_items` supplement the existing task/event tables. Header/status blobs
remain the served bytes. Schema5 roots refuse as `unsupported_layout`; no
migration/public creation path is introduced.

## Query and observation coverage

| Internal behavior | Pinned contract relationship and limit |
| --- | --- |
| IDs | ASCII `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`; exact wins, canonical32hex/hyphen UUID never prefix, prefix minimum8, case-sensitive literal underscore. Ambiguity at most5 candidate IDs plus truncation; public diagnostics/echo serialization belong to the separate ctl adapter. |
| Scope | Namespace precedes exact/prefix lookup; closed scopes bound100 supplied IDs. Actual managed role/ownership derivation remains absent. |
| Pending | Separate fixture-supplied exact ID presence, not a task record; never prefix candidate/list row. Typed Pending supplies no launch state, reservation/owner liveness or public status body. |
| Pages | Limit1..100; `(stamp,id)` descending keyset; independent supplied active observation and session filter up to256characters. Sort stamps are explicit fixture integers, not a new public timestamp parser. |
| Explicit IDs | Separate bounded request mode, deduplicated/request order; no silently ignored history filter/cursor. Missing IDs are lookup outcomes and omitted rows. This internal result does not supply public adapter error/drop/echo mapping; ctl has a separate mapping. |
| Stored status | Same scoped resolver, optional stored status bytes and independent recorded-status/active/exit/drain/owner facts. No supervision/provider replay, current process check or inferred successful delivery. |
| Evidence | Discovery/ownership/source completeness supplied explicitly. Unknown activity has separate incomplete evidence; matching-row count stays unknown when required evidence is incomplete. Count is not canonical total_active_count/root accounting. Pagination partial/more is independent of evidence completeness. |
| Bounds | Fixture header max2048bytes; status bytes also local2048byte bound, not the public full-status limit. Pages bounded128KiB with512reserve plus cursor/scope metadata accounting and Store limits. Oversized input/materialization refuses rather than falsely complete truncation. |
| Cursors | Internal typed store/dataset/query-filter/authorized-namespace binding; dataset replacement invalidates. Page-size changes preserve keyset semantics. No wire encoding/TTL/public reset/error contract is invented. |
| Snapshot | Dataset generation, resolution, rows and counts share one SQLite read transaction. The internal reader supports an already-open Store or ReadSession; ctl uses ReadSession, with no root init, explicit recovery, retention or execution. |

The internal byte budget accounts for materialized opaque bytes/metadata; it is
not a serialized public ctl response guarantee. Public ctl serialization is separate
in `hf_interfaces::ctl`.

All ordinary runtime `status` fields require the canonical supervision/control/
provider projections. An indexed `status` string is not proof of activity, observed
exit, terminal output drain, authority or completion. Unknown/null/false and absent
stored facts stay separate. Unknown source/ownership does not become complete
history, zero cost/count or direct authority. Workflow associations/root counts,
predecessor/successor discovery and compact group/title/repo/status/backend/since
filters are outside this internal query surface. The separate ctl task page
implements successor discovery; this does not add workflow accounting to R02-B.

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
the ctl producer has separate projections and validation under `hf_interfaces::ctl`.
No production task publisher is present in R02-B.

Dependency: exact serde_json1.0.154, defaults disabled/std only, permitted solely in
hf-interfaces with exact owner/version/features/kind enforcement. MIT OR Apache2.0,
declared MSRV1.71. Actual normal/build tree adds itoa/memchr/serde_core/zmij;
Cargo.lock also records inactive resolution entries, including derive tooling, but
no domain serialization/derive dependency or guard wildcard is enabled.

## Immutable fixture adoption and separate deltas

R02-W deliberately adopts the complete209file bundle (207fixtures plus2manifests)
under `tests/fixtures/contracts-v1` from corrected HarflowMCP0.0.10 source
`8a936de61b5f9f2fdff671b65b5384259267350b`. Manifest SHA256:
`ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906`.
[Consumer provenance](../tests/fixtures/contracts-v1.provenance.json) records every
old/new file hash, field change and adoption classification. Tests/builds use only
repository-owned bytes, without sibling lookup or network fixture fetching.

The original R02-B/R02-C proofs used199 fixtures from SB1-A
`b0185be855fd13acc94cb2d2008400d285d15df2`, manifest
`383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0`.
Those historical receipts are not relabelled as207-case runtime qualification.
Full-bundle hash verification is provenance evidence, not execution of every
operation. HarflowMCP remains the canonical contract owner until accepted O04.
Neither0.0.9 nor moving producer drafts are selected inputs.

## Required watcher foundation and remaining integration

[Task-list invalidation](task-list-watch.md) implements the assigned internal Rust
store/publication/wait seam and strict consumer mappings against corrected0.0.10.
A task metadata publication and its revision share one SQLite commit. Pending-only
fixture updates, owner/claim state, event records and liveness observations do not
publish task metadata invalidations. Watch responses remain hints, never task facts.

The consumer foundation establishes a boundary before snapshot, retains the returned
cursor before refresh, keeps one wait in flight, coalesces invalidations while busy
and reconciles lifecycle deadlines even after timeout. Actual Mac/Web integration,
fresh OS/runtime/authority qualification and public watch operations remain separate;
no app private persistence or agent-MCP fallback is introduced. See the
[R02-W receipt](migration-receipts/R02-W.md) for immutable inputs and exact proof scope.

B10/B11/B17, production catalog lifecycle, R03 capture/execution, scale/copy cost,
WAL-growth/checkpoint scheduling and broader fault/R14 gates remain outstanding.

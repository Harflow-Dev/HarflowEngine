# Internal stored event reads

R02-C is an internal, fixture-backed read seam. HarflowMCP remains the canonical
contract owner. That stage did not verify caller processes, ingest provider output,
supervise owners, expose a ctl command or implement the required watcher.
Current `harflow-ctl task-events` is implemented on macOS arm64 with a caller gate,
wire cursor encoding, log/change-token projection and document fitting. It uses
one read-only `ReadSession` snapshot under a shared lease, without a private copy
or per-call `quick_check`. This adds no ingestion or supervision. Watch
command/capability remains unavailable until E2; see the [partial E1 receipt](migration-receipts/E1.md).

## Inputs and ownership

The historical R02-C proof used SB1-A source
`b0185be855fd13acc94cb2d2008400d285d15df2` and199 fixtures, manifest
`383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0`.
R02-W now deliberately selects the full207-fixture corrected0.0.10 bundle from
`8a936de61b5f9f2fdff671b65b5384259267350b`, manifest
`ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906`.
[Consumer provenance](../tests/fixtures/contracts-v1.provenance.json) classifies every
version, capability and opaque cursor/token regeneration. Historical event writer
provenance remains producer-owned; adoption is not a rewrite of stored events or
retroactive qualification of prior receipts. No sibling runtime dependency or
public watcher host is introduced; see [watcher foundation](task-list-watch.md).

Domain owns pure query/filter/cursor and observation values. Application owns
constructor-injected authority and event-read ports. Storage owns optional SQLite
fixture tables, snapshot reads and explicit fixture DML. Interfaces owns consumer
JSON validation and pure fixture preparation using the existing serde_json seam.
R02-C added no supported transport; the current ctl producer is separate from
these consumer-only mappings.

## Contract decisions

| Concern | Required interpretation |
| --- | --- |
| Authority | The R02-C seam requires a trusted injected direct fixture grant before storage. The ctl caller gate separately uses marker/process/lineage observations; agent or undecidable callers are refused before task disclosure. Admission as NoAgentDetected is not a human credential or managed-ownership qualification. |
| Resolution | Exact/prefix and pending/unknown results remain separate from log readability; retention removal is not an empty readable history. |
| Filter | Default is the fixed known kind set except assistant_delta; all adds that kind. Unknown kinds are valid accounted records but filtered even by all. Consumer views preserve unknown kinds. |
| Order and work | Ascending output. Limit1–200 counts decoded records, including filtered/blank/malformed, rather than returned events. Oversized spans advance without consuming that count. Logical scan window is at most1048576 bytes. |
| Record bounds | Raw fixture classification refuses to parse lines over1000000 bytes. Normalized event bodies are at most16384 UTF8 bytes. Invalid UTF8 is malformed, not lossy text. |
| Cursor | Internal typed cursor binds store/task/filter/stream generation/direction/boundary and sequence/skip/resync context. The separate ctl codec wraps it in an opaque wire cursor; no Python private path/inode/anchor encoding is copied. |
| Continuity | Append preserves stream generation, including initially empty streams. Replacement, rewrite/truncation, root replacement or unavailable stream invalidates continuity. No time-based expiry or liveness claim. |
| Span and gaps | Accounted valid sequence values include filtered kinds. Skipped unreadable records influence gap reasons, including at adjacent-page boundaries. Unknown loss is not silently called unreadable. |
| Partial tail | Withhold until a complete newline-terminated record exists. A partial tail alone does not establish has_newer or finality. |
| Trimming | Remove the far side and recompute boundaries so dropped records remain reachable. Internal opaque-byte materialization accounting does not establish the public serialized-document budget. |
| Finality | Readable + observed-terminal capture + drained + last complete task_finished exactly at EOF. This does not prove complete history. Stored status alone proves none of these observations. |
| Other log states | Live capture=open; lost owner=incomplete; undecidable capture=unknown; unavailable stream=unavailable. Terminal/drained without final record may settle under independently supplied grace evidence, otherwise incomplete. |
| Versions | Outer ctl v1 uses the selected current release line; nested event v1 accepts syntactically valid historical writer versions independently. Unknown fields/string states remain intact. |
| Change tokens | The ctl adapter projects a sampled-source change token. It is not a watcher journal or authoritative equality/liveness token. |

## Qualification boundaries

Only explicit fixture initialization creates event tables under the existing
exclusive initializer. Current fresh roots use schema6; schema5 roots refuse as
`unsupported_layout`. Reads and fixture replacements do not upgrade old
roots or silently replace a future schema. Replacements and appends are test-input
operations, not product ingestion APIs. Facts about capture, drain, grace and stored
status are independent supplied observations. They do not become true merely because
an event row, terminal status or cursor exists.

R02-A2's cooperative local-filesystem locking, SQLite-controlled WAL recovery and
unsupported hostile-path/power-loss/platform/performance limits still apply.
Open/recover/probe retain capped whole-store qualification (64MiB); ctl reads use
the read session, which has no store-size cap of its own and may create/rewrite
`-shm`, with soft deadline acceptance. R02-B task dataset generations do not stand
in for an event stream's append continuity. Managed ownership, lifecycle adapters,
retention scheduling and public watcher durability remain separate work.
See the [R02-C receipt](migration-receipts/R02-C.md) for the delivered
API, exact tested cells and remaining gaps.

## Implemented internal representation

`EventQuery` contains a typed task identifier, limit, filter, oldest-forward choice
and optional typed cursor. Usage bounds are validated before fixture authority;
authority precedes every repository call. Task resolution precedes cursor shape,
task/filter binding and continuity validation. `EventReadResult` carries lookup,
resolved task, retained records, older/newer cursors and booleans, accounted span,
skipped counts, gaps, independent facts, decoded count and logical scanned bytes.
Pending, ambiguous, incomplete discovery and missing tasks retain the existing
bounded task lookup representation; none becomes a fabricated event history.

An `EventRecord` supplies start/end logical offsets including newline, class,
optional sequence/kind and normalized UTF8 body. Complete spans must be contiguous.
A withheld partial tail supplies length only. The explicit fixture caller asserts
that appending preserves the existing raw prefix; completion starts at the prior
complete end and covers the old partial tail. Stored raw bytes are not retained,
so this is an input trust boundary, not proof that arbitrary raw appends are safe.
The pure `prepare_line` helper produces matching sequence/kind/body values; storage
accepts already prepared fixture records and does not parse their JSON to prove that
relationship. A future ingestion adapter must establish both properties itself.

Sequences and offsets fit signed64-bit SQLite integers, kind metadata is at most
128 UTF8 bytes and optional recorded status is at most256 UTF8 bytes. These are
internal fixture limits, not additional canonical wire requirements. The helper
classifies larger sequence/kind inputs as malformed and uses strict serde JSON;
Python nonstandard numeric values and exotic float rendering are not qualified.
Known optional fields are type-checked by the consumer; original JSON remains
available, with missing/null/value distinctions and unknown strings intact.

The indexed reader uses named start/end indexes and bounded metadata projections
before allocating selected bodies. Filtered bodies are not allocated. The internal
materialization budget reserves1024 bytes plus per-record metadata and opaque body
costs, honors configured Store row/byte limits, and stops at the far side. It is not
a serialized ctl producer. `scanned_bytes` measures the primary logical byte window;
constant indexed EOF and newer-line lookups are additional metadata operations,
not actual raw-file byte reads or performance measurements. No full log replay is
required for a page. Fixture mutation validates the existing stream in bounded
iteration before replacement; this validation is not charged as a read-page scan.

EOF sequence is derived from a valid classified task_finished record ending at the
complete EOF, only when no partial tail exists and within the maximum raw record
length. A supplied contradictory EOF claim refuses before commit. Capture, drain,
grace and recorded task status remain separate fixture inputs. Grace is a supplied
bounded observation; the domain does not read the clock or turn elapsed time into
closure evidence. State final is independent of gap-free or complete history.

The consumer validates the entire received JSON byte length at262144, at most200
events, nested event size and cursor character limits. This bounds parsing of a
received document; the consumer itself does not implement producer fitting, canonical CLI flags,
error codes/exits, resolved_from echoes, observed_at sampling, change tokens or
wire cursor encoding. Those are separate ctl producer responsibilities now
implemented in `hf_interfaces::ctl`; hf-contracts remains an empty seam.

`eof_finished_seq` is an internal observation, not the public `log.final_seq` field:
it may be present while capture is unknown or drain is absent. The ctl producer
derives final_seq through `LogObject` rather than copying this observation alone.
The consumer preserves received log fields and does not manufacture this mapping.
EOF metadata is validated within the normalized fixture trust boundary; storage is
not an independent raw-event JSON verifier.

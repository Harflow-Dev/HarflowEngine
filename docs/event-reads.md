# Internal stored event reads

R02-C is an internal, fixture-backed read seam. HarflowMCP remains the canonical
contract owner. The host still refuses product operations. This seam does not
verify caller processes, ingest provider output, supervise owners, expose a ctl
command or implement the required watcher.

## Inputs and ownership

The immutable SB1-A input is HarflowMCP
`b0185be855fd13acc94cb2d2008400d285d15df2`, with S05 producer
`b69b5754a7b6496aaf5e3106116e302ea7a10675`. The full vendored bundle remains
199 fixtures, manifest SHA256
`383ecf7a9b234a1ece18eb15585398f08a77205c02a43cb2a928a2819f76fee0`.
The Python source clarifies behavior absent from individual examples; no sibling
runtime dependency is introduced. Neither 0.0.8 nor the in-progress 0.0.9 release
is adopted by this change. Watcher adoption has its own immutable readiness gate.

Domain owns pure query/filter/cursor and observation values. Application owns
constructor-injected authority and event-read ports. Storage owns optional SQLite
fixture tables, snapshot reads and explicit fixture DML. Interfaces owns consumer
JSON validation and pure fixture preparation using the existing serde_json seam.
No dependency or supported transport is added.

## Contract decisions

| Concern | Required interpretation |
| --- | --- |
| Authority | Only a trusted injected direct fixture grant reaches storage. Managed/blocked/undecidable callers disclose no task, prefix candidates or counts. This is not production caller verification. |
| Resolution | Exact/prefix and pending/unknown results remain separate from log readability; retention removal is not an empty readable history. |
| Filter | Default is the fixed known kind set except assistant_delta; all adds that kind. Unknown kinds are valid accounted records but filtered even by all. Consumer views preserve unknown kinds. |
| Order and work | Ascending output. Limit1–200 counts decoded records, including filtered/blank/malformed, rather than returned events. Oversized spans advance without consuming that count. Logical scan window is at most1048576 bytes. |
| Record bounds | Raw fixture classification refuses to parse lines over1000000 bytes. Normalized event bodies are at most16384 UTF8 bytes. Invalid UTF8 is malformed, not lossy text. |
| Cursor | Internal typed cursor binds store/task/filter/stream generation/direction/boundary and sequence/skip/resync context. No Python private path/inode/anchor encoding is copied and no public cursor format is exposed. |
| Continuity | Append preserves stream generation, including initially empty streams. Replacement, rewrite/truncation, root replacement or unavailable stream invalidates continuity. No time-based expiry or liveness claim. |
| Span and gaps | Accounted valid sequence values include filtered kinds. Skipped unreadable records influence gap reasons, including at adjacent-page boundaries. Unknown loss is not silently called unreadable. |
| Partial tail | Withhold until a complete newline-terminated record exists. A partial tail alone does not establish has_newer or finality. |
| Trimming | Remove the far side and recompute boundaries so dropped records remain reachable. Internal opaque-byte materialization accounting does not establish the public serialized-document budget. |
| Finality | Readable + observed-terminal capture + drained + last complete task_finished exactly at EOF. This does not prove complete history. Stored status alone proves none of these observations. |
| Other log states | Live capture=open; lost owner=incomplete; undecidable capture=unknown; unavailable stream=unavailable. Terminal/drained without final record may settle under independently supplied grace evidence, otherwise incomplete. |
| Versions | Outer ctl v1 uses the selected current release line; nested event v1 accepts syntactically valid historical writer versions independently. Unknown fields/string states remain intact. |
| Change tokens | No watcher journal or authoritative equality/liveness token is implemented. A future public adapter must map the canonical sampled-source semantics explicitly. |

## Qualification boundaries

Only explicit fixture initialization creates optional event tables under the
existing exclusive initializer. Reads and fixture replacements do not upgrade old
roots or silently replace a future schema. Replacements and appends are test-input
operations, not product ingestion APIs. Facts about capture, drain, grace and stored
status are independent supplied observations. They do not become true merely because
an event row, terminal status or cursor exists.

R02-A2's cooperative local-filesystem locking, bounded whole-store qualification,
SQLite-controlled WAL recovery and unsupported hostile-path/power-loss/platform/
performance limits still apply. R02-B task dataset generations do not stand in for
an event stream's append continuity. Public authority, lifecycle adapters, serialized
producer fitting, root discovery, retention scheduling and watcher durability remain
separate work. See the [R02-C receipt](migration-receipts/R02-C.md) for the delivered
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
received document; it does not implement producer fitting, canonical CLI flags,
error codes/exits, resolved_from echoes, observed_at sampling, change tokens or
wire cursor encoding. The host and hf-contracts remain unchanged.

`eof_finished_seq` is an internal observation, not the public `log.final_seq` field:
it may be present while capture is unknown or drain is absent. A future producer
must expose canonical final_seq only with the canonical final-state conditions.
The consumer preserves received log fields and does not manufacture this mapping.
EOF metadata is validated within the normalized fixture trust boundary; storage is
not an independent raw-event JSON verifier.

# ST06-A — published 0.0.11 contract baseline

Date: 2026-11-06. Status: baseline adoption delivered; operational steering
parity is not implemented or qualified. HarflowMCP remains canonical until accepted
O04 transfer. The owner authorized Rust adoption of published 0.0.11 separately
from the bounded Claude TLS experiment, which remains checkpointed and unqualified.

## Published identity

Public PyPI metadata and downloaded wheel/source archive were verified without
installation or artifact execution:

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `harflow_mcp-0.0.11-py3-none-any.whl` | 1368241 | `8ec421d1e21894f0fc9504f2d928eb02cef7c1d245fecd6fc5ba69204a346dfc` |
| `harflow_mcp-0.0.11.tar.gz` | 3501141 | `bdf02ad4a28dbc5d17c02e01a5006613e877171e246a873906bf3f2fa1641c4a` |

Both are unyanked in the observed [version-specific metadata](https://pypi.org/pypi/harflow-mcp/0.0.11/json).
All 222 wheel Python source files and all 293 source-archive `contracts/v1` files
match canonical checkout commit `6195e619499003301e4b20bacc3da86b5dbe371a` byte for
byte. The published [GitHub prerelease](https://github.com/Harflow-Dev/HarflowMCP/releases/tag/v0.0.11)
and its `v0.0.11` tag independently resolve to that exact commit. The prerelease
was published at 2026-11-06T10:22:57Z; it is not a Rust publication or owner acceptance.

The exact-head [main Engine checks](https://github.com/Harflow-Dev/HarflowMCP/actions/runs/42192019385)
and [Engine release](https://github.com/Harflow-Dev/HarflowMCP/actions/runs/42202239721)
were both observed completed/success. The duplicate tag-triggered
[Engine checks](https://github.com/Harflow-Dev/HarflowMCP/actions/runs/42202239575)
were still in progress at observation; no rerun or cancellation was performed.
An initial latest-release lookup excluded this prerelease and an unprefixed tag
lookup did not identify `v0.0.11`; the exact prefixed release/ref reads resolved
both. No blanket all-runs-green claim follows.

The canonical contract document has SHA-256
`850b4fded3f3b4732585902f7bcadf47cd7fdab5f733c49e32fe49aca20b70c2`.
The complete published fixture manifest is
`cd58a5ef8bae565513669dcf4d4ac9179ea327d5093de4461806d7be8dae1873`,
covering 291 specimens plus the two manifest files. Builds and tests use the
vendored bundle, without sibling-source dependencies.

## Audited delta and scope

The former 0.0.10 baseline contains 207 specimens, manifest
`ceb336216b8e5b0938f101b2a8b2ba5f85e3d939e281c446461499a2ae5c4906`.
Independent pointer audits agree on 70 unchanged, 110 product-version-only,
9 capability-list, 17 opaque cursor/generation/snapshot regeneration and one
usage-help change. No other prior specimen shape/value change was found.

The 84 additions comprise 45 steering, 33 conversation-delivery and six negative
capability-handshake specimens. The prior provenance is preserved byte for byte
under `tests/fixtures/provenance/contracts-v1-0.0.10.json`, SHA-256
`70d9ae6bc588609d25dbc74c563e38dcfc230a6197f5dc3140bbb169b6ec37a0`.
The current provenance records every specimen's old/new identity and classified
delta. No canonical specimen is regenerated or edited locally.

| Published family | Canonical behavior that later Rust implementation must preserve | Current Rust boundary |
| --- | --- | --- |
| Builder/orchestrator/worker steering | One finite response per explicit message; exact role, ownership, target and revisions; prose waits; strict current commits use normal validators | Specimens adopted; no operational steering ledger or workflow runtime |
| Shared windows and replay | Unlimited explicit message count, separate nonrefundable issuance, unchanged ordinary execution/decision/node-attempt allowances; epoch-scoped replay/expiry and explicit gaps | No durable steering accounting, compaction, replay or authority conformance yet |
| Prestart replan | Exact generation/configuration, positive never-started and release proof, same-assignment linked retry; uncertain/issued work cannot become replacement authority | No public Rust withdrawal/replan operation |
| Queue/Now delivery | Queue waits for the whole response and delivers FIFO; Now requires qualified activity/interrupt handling; claim/ack is not consumption; unknown cannot authorize replay | No operational R06 conversation queue or delivery runtime |
| Poststart replacement | Reserved, unadvertised `workflow-steering-poststart.v1`; private synthetic producer/helper candidates are not production support | Candidate/helper infrastructure remains separate and untouched |

Legacy schema-1 specimens retain historical finite quota observations. They do
not reinstate a lifetime steering quota: qualified schema-2/window behavior has
unlimited explicit messages and finite per-response caps (at most eight, honoring
a smaller configured positive cap). No message consumes or resets normal
execution turns, node attempts or decision attempts. Runtime permissions and
configured resources remain effective.

## Dependencies and remaining qualification

This stage changes fixture data, provenance and verification only. Existing
consumer-only task/event/watch adapters remain data mappings. The scaffold does
not advertise the producer's capabilities or select a new runtime version.

Operational ST06 requires the R06 durable continuation/message foundation and
R10/R11 immutable workflow definitions, role validators, scheduling checkpoints,
nested/parallel execution and recovery authority. Those production foundations
are absent; unrelated private owner-process/storage mechanics do not substitute
for them. Full state-machine, restart, authority, idempotency, accounting, race,
backend and native/Rust conformance remain ST06/R11/R14 work. Fixture integrity
and passing existing data adapters do not qualify those semantics or acceptance.
No Python persistence is cloned, and no alternative runtime is introduced merely
to bypass those dependencies.

## Verification

The primary independently verified all 293 vendored files against the downloaded
source archive and all 291 specimen hashes against its pinned manifest. The
preserved 0.0.10 provenance hash also matches. All 75 `hf-interfaces` Rust tests
passed against the updated baseline, including the existing task/event/watch
adapters. All 25 Python tests passed, including five focused bundle/audit checks.
After tightening old/new audit identity checks, those five checks passed again.
Formatting, workspace check, interface all-target Clippy, architecture/legacy
guards and whitespace checks passed. This is scoped local evidence; the unrelated
peer ST04 all-workspace Clippy limitation is not repaired or included in that claim.

Independent plan review and final review closed clean. The final reviewer checked
all 293 published files, 291 current hashes, 207 prior hashes and every changed
JSON Pointer value against the pre-adoption commit. All 17 protected peer files
remain byte-identical. Peer-owned migration tracker and ST04 helper files are
excluded. No helper/vendor/provider execution, credential access,
installation, production admission, cutover, push or release is part of this stage.

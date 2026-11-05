# Owner-runtime candidate receipt

This records a cohesive private implementation stage, not production acceptance,
public execution support, backend qualification or release authorization.

## Provenance

- Canonical owner: HarflowMCP, local commit
  `4f5b86d27d249094d7378c84029a94e2aaec966d`.
- Candidate component: `contracts/candidates/owner-runtime-v1`, manifest SHA-256
  `7df0c93a17b185620e35fcf0d0a3e060174ff0445318ed505dc7e4573a08c6d6`.
- Rust base: `cdde450deff0095d2a8c079bcaf6d690a42b64ba`.
- Supported MCP 0.0.10/full207 input remains separate and unchanged; no incidental
  adoption of concurrent steering aggregates or consumer fixtures.
- Rust toolchain remains pinned to 1.99.1; storage schema remains 5.

The canonical component comprises two normative specifications, deterministic
standalone fixture tooling and 64 synthetic specimens. It adds no Python owner or
storage runtime, public capability or supported contract replacement. Known Process fields define identity comparison; unknown optional metadata does
not alter it. Fixture tooling rejects hardlinks and uses atomic replacement. The
final standalone canonical fixture suite passed 10 tests before the local commit;
the Rust source review verified those immutable behaviors.

## Implementation

See [the maintained design](../owner-runtime.md) for policy and authority bounds.
Rust adds pure policy decisions, strict bounded profile parsing, digest-bound
static preparation, a split-phase one-command coordinator, a bounded binary codec,
an independent root-only hold, immutable deny-only writer provenance, incremental
writer transport and private fixed-helper owner/writer composition. The main
owner holds no SQLite connection. Writer and agent retain their original owned
handles and sole wait authority. Physical quiescence is distinct from durable
acknowledgment and resource release.

The architecture allowlist now admits boxed pure values while inspecting their
inner types; boxed filesystem/process effects remain rejected. The legacy-name
scanner checks bytes so intentionally malformed UTF-8 specimens remain scanned
without a decoding exception.

Acknowledgments bind stable raw identities; expiry is checked before sending.
EOF requires independently tested reader, data and input reconciliation and an
empty append. Actor errors permanently poison admission while physical draining
continues; post-join FIFO accounting remains required. Unix sidecar code is gated
with explicit unsupported refusal elsewhere; local macOS checks do not establish
a Windows build.

## Validation

Final local macOS arm64 verification on Rust/cargo 1.99.1:

- Rust workspace: 350 passed, zero failed, three expected helper/probe exclusions.
- Explicit old/new protocol refusal probe: one passed; old executable SHA-256
  `79d5642b05861b2467fe20fd1fec37b7b9b08deb8624a84ac22533fb7d43382b`.
- Python repository checks: 22 passed. Formatting, locked workspace check,
  all-target Clippy with warnings denied, architecture, legacy-name and whitespace
  guards passed.
- Exact candidate classification covers all 64 pinned synthetic specimens.
- An actual gated writer leaves owner progress, original-agent exit and reader
  join observable while the data append remains unacknowledged; releasing the gate
  completes reconciliation and exact raw-output verification.
- Exact retained-owner kill/reap leaves the writer's root lease busy. After gate
  release, positive read-only terminal observations for both original descendants
  and an available root lock precede temporary-root teardown. Unknown observation
  never counts as death; unresolved failure cleanup aborts before teardown.
- Independent source review completed. The final test prerequisite oracle was
  checked separately for each missing reconciliation fact before the final run.

The synthetic corpus and fixed-helper checks do not substitute for measured
production admission or actual platform/storage-fault qualification. Hosted CI,
Linux and Windows execution were not observed by this local verification.

## Outstanding gates

Production caller lineage and grants, stock backend/version/configuration evidence,
measured OS-enforced protected paths, descendant containment, production numeric
budgets, additional platform support and actual storage-fault behavior remain
outstanding. Unsupported production paths must refuse. No replay, automatic
adoption, replacement, release or cancellation authority is introduced.

Accepted steering semantics also target Rust; this stage binds context and refuses
result/acknowledgment substitutions for control authority. It does not implicitly
start the entire subsequent backend/steering stage. Tracker-owned migration files
remain untouched by this stage. No push, release, acquisition or installation is
part of this receipt.

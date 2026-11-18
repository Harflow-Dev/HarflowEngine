# R10-A workflow-definition decisions

Owner decisions for the Rust port of workflow-definition validation, references,
save/delete and run snapshots (R10-A1 and R10-A2). Recorded in the repository on
2026-11-18 so the receipts can cite them; the source of record is the R10-A plan
(`.claude-work/engine/stream2/r10a-plan.md` section 11, outside this repository),
whose status line reads: the owner accepted all twelve recommendations as written
(2026-11-15). The R10-A2 addendum (plan section 12, 2026-11-16) was authorized with
R10-A2.

The port deliberately fixes Python defects rather than copying them. Where a
decision below changes an outcome, the Rust result is the accepted behaviour and the
Python outcome is kept only as evidence in the parity fixtures.

## Accepted decisions (2026-11-15)

| # | Subject | Accepted behaviour |
| --- | --- | --- |
| D1 | Malformed values that crash Python (`TypeError`, `OverflowError`, `UnknownBackend` escaping `valid:false`) | Refused as ordinary validation errors with the existing message for the rule involved. |
| D2 | Definitions that validate but can never be read back after saving (for example `network: 1`, `routing_mode: null`, NaN, `role` on a Start node) | Refused at validate and save with the bare decode reason. Fields the store overwrites are never refused. |
| D3 | Lone surrogate characters in JSON text | Refused. |
| D4 | Outputs whose Python order depends on the hash seed: which error message wins, and `subtree` key order (P9, P13) | Deterministic: node-list order for errors, parent-tree insertion order for `subtree`. |
| D5 | `resources` given as an object or a scalar | Refused with `resource names must be a list of names`. |
| D6 | A new agent node that declares resources without an explicit `execution_mode` (P5). Python defaults it to `prefer_subagent` and then fails save with `prefer_subagent nodes cannot declare resources: <id>`. | Save defaults that node to `headless`. Existing nodes keep their stored mode; an existing `prefer_subagent` node that gains resources is still refused. |
| D7 | Size bounds Python lacks | 1 MiB of definition text, 1000 nodes including on-demand nodes, 4000 connections, JSON depth 64. |
| D8 | `observe_repos` paths resolved through symlinks at save time | Kept for parity, behind a port; revisit at R13 import. |
| D9 | Owner of the backend capability table, command-pattern bounds and Unicode category table | R12 owns them; R10-B composes them, with test tables until then. |
| D10 | Where Rust stores definitions | A SQLite table in the Rust store, in Rust's own root. |
| D11 | `product_version` in definition envelopes | Rust's own version string stays the persisted identity; no Python reader compatibility is required. |
| D12 | Error messages | Byte-identical to Python, apart from the decisions above and the addendum below. |

## R10-A2 addendum (2026-11-16)

1. Save-time qualification equivalence: for every valid corpus case, validate-time
   qualification equals stored qualification of the document save would write.
2. Revision digit limit: incrementing a stored revision past Python's 4300-digit
   integer limit makes Python raise while hashing during resolve, with
   `Exceeds the limit (4300 digits) for integer string conversion; use
   sys.set_int_max_str_digits() to increase the limit`. Rust refuses before writing
   and reports the decode reason a reader of the rendered document would give
   (`... value has 4301 digits; ...`). This is an accepted message divergence from
   D12. Rust checks the carry before listing the catalog, so when the store also
   holds a corrupt sibling or a duplicate identity, Rust reports the integer refusal
   where Python would report the catalog error.
3. `identity.rs` from A1 is extended, not recreated.
4. Bounded work: reference resolution and `subtree` stay iterative and bounded by D7
   per definition and by the store's contents; recursion is ported to explicit
   stacks, with work-bound tests.
5. Depth guards for `Validator::candidate` and `migrate::project_definition` stay
   with R10-B.

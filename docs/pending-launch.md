# Pending launch discovery

`task-pending-launch.v1` (store-gated, advertised first in the capability list, as in the
reference) adds two direct-human commands that list launches which have no task record yet.
Receipts are display evidence, never task status. On a host without store reads both are
unknown commands (a usage error, exit 2).

- `task-pending-page [--limit N] [--cursor C]`: a bounded page, 1 to 100 rows (default 50).
- `task-pending-status --task-id ID`: one launch by its exact full id. A prefix is never
  resolved.

Both print the result under `--json` as the envelope `{"result": ...}`, and the bare result
indented without it. Reference: HarflowMCP `0ac7e3c` (`task_pending_reads.py`, `ctl.py`).

## Order of checks

Usage, then in this order (a dedicated path; `enter_checked` is unchanged for other commands):

1. Environment markers: any present marker variable, an empty value included, gives
   `agent_caller` with `check: direct_caller_required` before the root is looked at.
2. Root availability: the root is never initialized. A missing, empty, foreign or incompatible
   root gives `pending_observation_unavailable` (no `check`).
3. The direct-only caller gate, inside one store snapshot: every ordinary record is checked
   first (below), then the process view and the related unfinished tasks decide
   `agent_caller` or `caller_undecidable`, both with `check: direct_caller_required`.
4. Limit (`invalid_pending_limit`, `limit_1_to_100`), then cursor (`pending_cursor_invalid`,
   `cursor_shape`), or for the status command the task id (`invalid_task_id`,
   `exact_task_id_required`).
5. The cursor binding and the namespace.

Every error of this family carries `data: {coverage: "unknown", check}` except the
unavailable-root error, which carries `data: {coverage: "unknown"}`.

### Candidate check

The reference qualifies the complete ordinary-record source before it trusts a negative
caller detection; an identity that is invalid, a record that disappears or an exhausted
budget make the caller undecidable. Here one statement runs over every ordinary record in the
same snapshot. The caller is undecidable when the dataset coverage is incomplete, any record is
not valid, any record id is not a task identity, the records and receipts together exceed the
entry or name-byte bound (the reference applies those to the one directory that holds them),
or the request's read budget or deadline runs out. Any store failure while the caller is being
established gives the same answer. Records are charged by their raw stored length (header and
status documents). No launch metadata is read before the check passes. Because that bound is
met here first, a page can report `namespace_bound` only when the namespace grows between the
check and the namespace read, which one snapshot rules out.

## Launch namespace and rows

The namespace is every row of `launch_attempts`, keyed by the receipt filename stem; the
receipt filename is `<name>.launch.json`. Rows whose name is not a task identity are counted
(`invalid_name_count`) and never examined or echoed.

- `namespace_revision` is SHA-256 over the sorted filenames, each followed by a NUL byte,
  computed in `hf-storage`. The empty namespace is the SHA-256 of nothing.
- Rows are ordered and resumed by the full filename. The reference orders by filename but
  resumes by the bare id, which skips an id that is a prefix of the previous one (`ab-c`
  sorts before `ab`). Here both use one key, so nothing is skipped. The conformance case
  `pending-launch.prefix_nested_ids` passes only with `--delta pending-prefix-paging`.
- A row is examined as follows. A qualified ordinary record with the same id wins
  (`recorded`; filtered from pages). A receipt that is not a launch document is `refused`
  with its stored `code` and `check`. A `spawned` or `settled` receipt without a record is
  `unknown` (`pending_record_inconsistent`, `postrecord_source_without_record`; filtered from
  pages). Text that is not valid UTF-8 is refused as `field_shape` /
  `display_text_encoding`. Resource names that fail the reference's rules (1 to 16 distinct
  names of lowercase letters, digits and `. _ : -`) are refused as `field_shape` /
  `resource_names`. Anything else is `pending`.
- `cancel_support` is `supported` exactly when the state is not `not_started` and the receipt
  holds a `resources` or a `worktree` object. A receipt whose object lacks its member
  (`resources: {}`, `worktree: {}`) is refused as `field_shape` / `field_shape`, and an empty
  name list (`resources: {"names": []}`) as `field_shape` / `resource_names`. Ordinary `status`
  still reports the second as `task_pending_launch`.
- The page keeps an empty `not_started_reason` as `""`, unlike ordinary `status`, which omits
  it.
- Text is cut on a character boundary: `repo_path` at 4096 bytes, other text at 1024, resource
  names at 256 bytes, at most 100 names. `metadata_truncated` says any cut happened.

## Paging

A page examines at most 100 candidates; filtered ones count as examined and the cursor still
advances past them. It stops earlier at `limit` listed rows, at the read budget
(`bound: read_budget`) or at the response bound (`bound: response_bytes`, items kept below
128 KiB minus 8 KiB). A bound hit before any candidate was examined is an error
(`pending_observation_bound`); with some examined it is a partial page with
`coverage: unknown` and a cursor. Uncertainty is sticky: a refused row, an invalid name or a
bound makes the page and every cursor from it `unknown`. Stopping at the examined bound is not
uncertainty. The finished result must stay below 128 KiB minus 1 KiB.

The cursor is `urlsafe_b64encode` of compact JSON with sorted keys, at most 4096 bytes:
`{v: 1, scope, root, identity, revision, last, limit, uncertain}`, the reference's shape.
`root` is the SHA-256 of the selected state root, `revision` the namespace revision, `last` the
task id of the last candidate examined, and `identity` four whole numbers taken from the store
identity. Shape errors are `pending_cursor_invalid` / `cursor_shape`; another root or limit is
`cursor_scope`; another store or namespace revision is `pending_cursor_expired` /
`namespace_changed`; an unknown `last` is `cursor_position`. A change of row content that
keeps the set of names does not expire a cursor; adding or removing a name does.

## Bounds

| Bound | Value | Notes |
| --- | --- | --- |
| Namespace entries | 65,536 | `pending_observation_bound` / `namespace_bound` |
| Namespace name bytes | 8 MiB | filename bytes, suffix included |
| Read budget | 8 MiB | each ordinary record by its raw stored length, each receipt read 65,537 bytes |
| Examined per page | 100 | |
| Response | 128 KiB | items 8 KiB, finished result 1 KiB below it |

In debug builds only, `HARFLOW_PENDING_ENTRIES`, `HARFLOW_PENDING_NAME_BYTES` and
`HARFLOW_PENDING_READ_BYTES` lower the first three (never raise them) so a test can reach them
on a small store. Rows are read one at a time, after their name, and paid for first: a record
wins at its stored size with no receipt read, and a receipt is fetched only once its charge
fits, so a page that is full or out of budget decodes no later receipt.
A release build never reads them and no public flag exposes them.

## Ordinary reads of a launch receipt

Ordinary `status`, `status-batch` and `task-events` of an id with no record whose launch
receipt does not qualify as a launch document return the reference's document error: the
stored refusal code (for example `field_shape`) and the message
`<state root>/tasks/<id>.launch.json [<check>]: <reason>`, with no `data`. Such a receipt is no
longer `unknown_task`. A qualified pending-kind receipt still gives `task_pending_launch`.

## Deltas from the reference

- Namespace entries and name bytes count launch rows (valid and invalid names); the reference
  also counts every other directory entry. Rust has no other entries in the namespace.
- A candidate won by an ordinary record is charged the record's stored header and status
  documents (Python: the record file); a receipt read is charged 65,537 bytes before the
  receipt is fetched. A receipt whose raw text columns exceed 65,536 bytes is refused as
  `oversize` without being decoded. The reference measures the receipt file, whose JSON keys,
  escaping, whitespace and omitted fields make its size differ from the stored columns, so the
  two thresholds are not byte-identical.
- The vendored `ctl-task-pending-bound.json` (`namespace_bound` on a page) cannot be compared
  byte for byte at RT5: the caller check meets the same bound first, and a snapshot gives no
  window for the namespace to grow after it.
- The caller check is one statement over the records in the snapshot. It cannot see a record
  that disappears or changes during the read, and the namespace re-check after the page is
  not needed: the whole command runs in one snapshot. `namespace_recheck_budget` is never
  produced.
- The cursor `identity` is derived from the store generation instead of device and inode
  numbers; its values differ from the reference's, its shape and role do not. `root` is the
  hash of the lexical state root, not of the resolved path.
- Row order and resumption use the full filename (above); the reference's bare-id skip does
  not occur (`pending-prefix-paging`).
- The document error of ordinary reads names a notional receipt path under the state root,
  and uses one reason text per check; no receipt file exists.
- A store that cannot be opened (busy, incompatible, replaced) reports
  `pending_observation_unavailable` without a check, or `pending_observation_bound` /
  `read_budget` once the read deadline has passed. A failure after the gate carries the
  reference's check: `namespace_unavailable` for the binding, namespace and cursor position,
  `source_unavailable` for the rows.
- `observed_at` is null when the host has no clock; the reference always has one.
- A damaged ordinary record with the same id as a receipt cannot reach the row lookup: it
  makes the caller undecidable first, so the reference's `task_record_qualification` refusal
  of a row is not produced.
- The entry and name-byte bound of the caller check counts record rows and receipt rows; the
  reference counts every file in the tasks directory.
- Ordinary reads that never read receipts in the reference (`task-list-page --task-ids`,
  artifacts, recovery activity) treat an unqualified receipt like a pending launch: the id is
  dropped, or reported as an unknown task. `task-prompt`, `task-events`, status and successor
  discovery of that id return the document error.

"""Field policy and comparison engine of the v0.0.13 fixture byte comparison (plan R3-D).

A scenario runs the real harflow-ctl over a root seeded with hf-fixture-seed and hands the
document to `compare`, which applies the named substitutions it is given and then requires the
canonical bytes of the document to equal the vendored fixture. Nothing else is allowed to differ.
Every substitution is registered in SUBSTITUTIONS with its reason; docs/fixture-byte-comparison.md
lists the same names and the test module checks that the two agree.
"""

import base64
import datetime
import hashlib
import json
import re
from pathlib import Path

from ctl_support import FIXTURES

STAMP_PATTERN = re.compile(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d{6})?\+00:00$")
RUST_VERSION = "0.0.13+rust"

SUBSTITUTIONS = {
    "product_version": "Rust advertises 0.0.13+rust; the fixture says 0.0.13.",
    "observed_at": "Reader-clock stamps: the Rust reader stamps the real clock (checked as a recent UTC "
                   "stamp); the fixture carries the fixed instant of the Python clock.",
    "pending_cursor": "Opaque pending cursor: decoded and round-tripped; its root digest and identity "
                      "binding (store generation, lexical root) are replaced by the fixture's.",
    "artifact_cursor": "Opaque artifact cursor: decoded and round-tripped; its revision (a random per-store "
                       "token, checked against the stored manifest revision, not the reference's content "
                       "digest) is replaced by the fixture's.",
    "root_path": "The scratch root and repository live under a temporary directory; that prefix is "
                 "written /workspace, as the Python generators did.",
    "artifact_path": "The port places every artifact at <repo>/out/<name>; each path is replaced by the "
                     "fixture's path for the same artifact id (the Python engine-output path is in its "
                     "scratch directory).",
    "artifact_observation": "Present targets are real files: their registered device, inode, size and "
                            "mtime_ns are the real stat, replaced by the fixture's stat constants.",
    "status_subset": "Task bodies are compared on the exact key intersection; the fixture keys Rust does not "
                     "emit (SEED_ABSENT and RUST_UNPROJECTED) and the Rust keys the fixture lacks (RUST_ONLY) are asserted as "
                     "exact enumerated sets and removed before the byte comparison.",
    "session_id": "Known difference: Rust cannot represent a null session id (the store column is NOT NULL, "
                  "TaskFacts.session_id is a String), so a task without one reports \"\" where Python "
                  "reports null.",
}
# Python task keys that are stored record data: Rust serves stored blobs verbatim, but the seed port
# writes no value for these, so they are absent only because of the seed. Per command.
SEED_ABSENT = {
    "status": frozenset((
        "allowed_commands", "auto_continue", "base_commit", "caller_approvals", "caller_write_approvals",
        "depth", "dispatch_origin", "enforcement", "extra_path_certification", "extra_writable_paths",
        "interaction_owner", "lineage_detected", "live_input", "max_depth", "max_turns", "outcome",
        "parent_task_id", "reasoning_effort", "root_task_id", "spawned_by", "start_dirty",
    )),
    "brief": frozenset(("dispatch_origin", "parent_task_id", "root_task_id")),
}
_LIVE = "read-time observation of the reader or live owner; Rust has no projection"
_LOG = "parsed from the raw stream log at read time; Rust has no stream-log projection"
_VIEW = "read-time view of a side store (inbox, follow-ups, approvals, transcript); Rust has no projection"
_PATH = "path derived from the Python log directory; the Rust store has no such file"
# Python keys whose value is a read-time projection rather than stored record data, so Rust cannot
# serve them faithfully even if a value were stored. The reason is the one-line account.
UNPROJECTED_REASONS = {
    "approval_summary": _VIEW, "available_tool_count": _LOG, "background_idle_seconds": "resolved from "
    "configuration at read time; Rust has no projection", "denial_categories": _LOG, "events_log": _PATH,
    "failure_diagnostic": _LOG, "input_channel": _LIVE, "is_error": _LOG,
    "last_captured_output_at": _LIVE, "last_output_tail": _LOG, "mcp_servers": _LOG, "messages": _VIEW,
    "metadata_persisted": "constant added by the Python read path; Rust has no such member",
    "note": "Python's supervision text; Rust removes a stored note (task_docs.rs) and uses its own "
            "classifier, which gives no note for a completed task",
    "notices": _LOG, "num_turns": _LOG,
    "observation_note": "Python's observer-continuity text; Rust gives its own text only when a parent "
                        "observer is known (read_docs.rs)",
    "output_capture": _LIVE, "owned_by_live_server": _LIVE, "pending_messages": _VIEW,
    "permission_denials": _LOG, "queued_followup_count": _VIEW, "queued_followups": _VIEW,
    "raw_stream_log": _PATH, "recovery_activity": _VIEW,
    "responding_server": "identity of the responding Python process; Rust has no such process identity",
    "resume_command": "derived from backend, session and path authority at read time; Rust has no projection",
    "summary": _LOG, "supervision": _LIVE, "total_cost_usd": _LOG, "usage": _LOG,
}
RUST_UNPROJECTED = {
    "status": frozenset(UNPROJECTED_REASONS),
    "brief": frozenset((
        "background_idle_seconds", "input_channel", "last_captured_output_at", "note", "observation_note",
        "output_capture", "owned_by_live_server", "responding_server", "supervision",
    )),
}
# Keys Rust emits that Python's status, brief and batch rows do not: a real wire difference of the
# projection, not a seed limit. Identical for `status`, `status --brief` and `status-batch`.
RUST_ONLY = frozenset((
    "finished_at", "needs_reconciliation", "observed_exit", "persisted_status", "process_identity_state",
))


def canonical(value):
    text = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    return (text + "\n").encode("utf-8")


def fixture_bytes(name):
    return (FIXTURES / name).read_bytes()


def fixture_doc(name):
    return json.loads(fixture_bytes(name))


def at(doc, path):
    for key in path:
        doc = doc[key]
    return doc


def put(doc, path, value):
    at(doc, path[:-1])[path[-1]] = value


def recent(case, text):
    case.assertRegex(text, STAMP_PATTERN)
    moment = datetime.datetime.fromisoformat(text)
    age = datetime.datetime.now(datetime.timezone.utc) - moment
    case.assertLess(abs(age.total_seconds()), 600, text)


def tagged(rule, *names):
    """Mark `rule` with the SUBSTITUTIONS it applies; `compare` refuses an unregistered name."""
    rule.substitutions = names
    return rule


def product_version(case, doc, expected):
    case.assertEqual(doc["product_version"], RUST_VERSION)
    doc["product_version"] = expected["product_version"]


product_version.substitutions = ("product_version",)


def observed(*paths):
    """Replace the reader-clock stamps at `paths` by the fixture's."""
    def rule(case, doc, expected):
        for path in paths:
            recent(case, at(doc, path))
            put(doc, path, at(expected, path))
    return tagged(rule, "observed_at")


def sha256_text(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def pending_cursor(case, state, path, followup):
    """The cursor at `path` is the reference's compact sorted JSON in padded urlsafe base64.

    Re-encoding the decoded cursor must reproduce it exactly. Its root digest is the SHA-256 of the
    lexical state root and its identity is the store generation (third line of store.identity) as
    four 32-bit integers. Both are then replaced by the fixture's and the re-encoded bytes must equal
    the fixture's cursor. `followup(cursor)` proves the original cursor resumes.
    """
    def encode(value):
        text = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        return base64.urlsafe_b64encode(text).decode()

    def rule(case, doc, expected):
        cursor, wanted = at(doc, path), at(expected, path)
        value = json.loads(base64.urlsafe_b64decode(cursor))
        case.assertEqual(encode(value), cursor)
        case.assertEqual(value["root"], sha256_text(state()))
        generation = (Path(state()) / "store.identity").read_text(encoding="ascii").splitlines()[2]
        case.assertEqual(value["identity"], [int(generation[n:n + 8], 16) for n in range(0, 32, 8)])
        followup(cursor)
        pinned = json.loads(base64.urlsafe_b64decode(wanted))
        value.update(root=pinned["root"], identity=pinned["identity"])
        put(doc, path, encode(value))
        case.assertEqual(at(doc, path), wanted)
    return tagged(rule, "pending_cursor")


def artifact_cursor(followup, revision, path=("result", "next_cursor")):
    """`[task, revision, offset]` in padded urlsafe base64 of `json.dumps` with its default spacing.

    Re-encoding must reproduce the cursor. The revision is a random 32-hex token of the store,
    equal to `revision()`, the manifest revision stored for the task.
    """
    def encode(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).decode()

    def rule(case, doc, expected):
        cursor, wanted = at(doc, path), at(expected, path)
        value = json.loads(base64.urlsafe_b64decode(cursor))
        pinned = json.loads(base64.urlsafe_b64decode(wanted))
        case.assertEqual(encode(value), cursor)
        case.assertEqual((value[0], value[2], len(value)), (pinned[0], pinned[2], 3))
        case.assertRegex(value[1], r"^[0-9a-f]{32}$")
        case.assertEqual(value[1], revision())
        followup(cursor)
        value[1] = pinned[1]
        put(doc, path, encode(value))
        case.assertEqual(at(doc, path), wanted)
    return tagged(rule, "artifact_cursor")


def root_path(base):
    """Write the scratch directory prefix as /workspace in every string, keys included."""
    def walk(value):
        if isinstance(value, dict):
            return {walk(key): walk(item) for key, item in value.items()}
        if isinstance(value, list):
            return [walk(item) for item in value]
        return value.replace(base, "/workspace") if isinstance(value, str) else value

    def rule(case, doc, expected):
        case.assertIn(base, json.dumps(doc))
        doc.update(walk(doc))
    return tagged(rule, "root_path")


def status_subset(kind, *task_paths):
    """Compare each task body on the exact key intersection.

    `set(fixture) - set(rust)` must be SEED_ABSENT[kind] | RUST_UNPROJECTED[kind] and `set(rust) - set(fixture)` must be
    RUST_ONLY, both exactly; those keys are removed and every remaining value must be equal, apart
    from the session_id substitution.
    """
    def rule(case, doc, expected):
        for path in task_paths:
            got, want = at(doc, path), at(expected, path)
            case.assertEqual(set(want) - set(got), SEED_ABSENT[kind] | RUST_UNPROJECTED[kind], path)
            case.assertEqual(set(got) - set(want), RUST_ONLY, path)
            case.assertIsNone(want["session_id"])
            case.assertEqual(got["session_id"], "")
            got["session_id"] = None
            put(doc, path, {key: got[key] for key in want if key in got})
            put(expected, path, {key: want[key] for key in want if key in got})
    rule.partial = True
    return tagged(rule, "status_subset", "session_id")


def compare(case, name, done, rules, exit_code=None):
    """The ctl output, under `rules`, has exactly the bytes of fixture `name`."""
    manifest_code = json.loads(fixture_bytes("manifest.json"))["cases"][name].get("exit_code")
    case.assertEqual(done.returncode, manifest_code if exit_code is None else exit_code, done.stderr)
    # The ctl prints the reference's spelling (json.dumps, sorted keys, default separators); the
    # fixtures hold the canonical compact form of the same document.
    case.assertEqual(done.stdout, json.dumps(json.loads(done.stdout), sort_keys=True) + "\n", name)
    doc, expected = json.loads(done.stdout), fixture_doc(name)
    rules = [product_version, *rules]
    for rule in rules:
        case.assertTrue(set(rule.substitutions) <= set(SUBSTITUTIONS), rule.substitutions)
        rule(case, doc, expected)
    case.assertEqual(canonical(doc), canonical(expected), name)
    if not any(getattr(rule, "partial", False) for rule in rules):
        case.assertEqual(canonical(doc), fixture_bytes(name), name)


def artifact_paths(prefix, *names):
    """Replace item paths, in order, by the fixture's after checking the port's `<repo>/out/<name>`."""
    def rule(case, doc, expected):
        for index, name in enumerate(names):
            path = ("result", "items", index, "path")
            case.assertEqual(at(doc, path), f"{prefix}/out/{name}")
            put(doc, path, at(expected, path))
    return tagged(rule, "artifact_path")


def artifact_observations(stats, *indexes):
    """Present targets: the registered observation is the real stat taken when it was registered;
    the fixture's constants replace it."""
    def rule(case, doc, expected):
        case.assertEqual(len(stats), len(indexes))
        for index, info in zip(indexes, stats):
            where = ("result", "items", index, "registered_observation")
            case.assertEqual(
                at(doc, where),
                {"device": info.st_dev, "inode": info.st_ino, "size": info.st_size, "mtime_ns": info.st_mtime_ns},
            )
            put(doc, where, at(expected, where))
    return tagged(rule, "artifact_observation")


def provenance_added():
    path = Path(FIXTURES).parent / "contracts-v1.provenance.json"
    entries = json.loads(path.read_bytes())["files"]
    return sorted(entry["file"] for entry in entries if entry["classification"] == "added")

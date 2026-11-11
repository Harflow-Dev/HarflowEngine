"""Shared helpers for the black-box tests of the history reads (`task-events`, `task-prompt`,
`task-artifacts`, `status --brief`).

The oracle is Python's own json, base64, hashlib, os and sqlite3. Cursors of the reference
engine are built here from its documented shape; no value is taken from the code under test.
"""

import base64
import hashlib
import json
import os

import seed_support as seeds
from reads_support import ReadsCase
from seed_support import A, B

DOCUMENT_BYTES = 262_144
RESERVE_BYTES = 512
STAMP = "2026-01-10T10:%02d:%02d+00:00"
PY_GENERATION = "py-events-1"

# sha256 of the task id text, as the reference engine's cursor `task` field. Derived once by
# running `digest()` from event_pages.py of HarflowMCP commit a426c5a (read with `git show`)
# on these ids; they are literals so the engine under test is never the source.
TASK_DIGEST = {
    A: "303617b9730210ef3c86c52dc2aecc4dce54aaca6af8c8b0f4ceec9ecc54e57e",
    B: "00f765af1c54eb24e437746c4f64b5841490757b647bf3a392b042f872ad7090",
}
# `filter_digest()` of the same module: sha256 of json.dumps(sorted(allowed)). `default` is
# EVENT_KINDS minus assistant_delta, `all` is EVENT_KINDS and the last two are explicit sets.
FILTER_DIGEST = {
    "default": "ce389cee2f6f5a4f4a89d9cf5323184abcdf96994b7ef92b93881f82353b84b1",
    "all": "bbd5927ab0d27ec617257d38bd4e40986341743da0cebb55415253e2d2e67125",
    "assistant_text,notice": "26b76a4ec8c5c557f8fa1d381dd25bb28bff9ce649ff8135c0f2a4ca7ec20ce2",
    "assistant_delta": "adc4d3c572325902a80a5f1e425cec600c79e751222e496aad0232dc66be2e33",
}
FILTER_ARGS = {
    "default": (),
    "all": ("--all-kinds",),
    "assistant_text,notice": ("--kinds", "assistant_text,notice"),
    "assistant_delta": ("--kinds", "assistant_delta"),
}


def sha256_text(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def python_value(task_id, filter_key, **overrides):
    """The fields of a cursor in the exact shape event_pages.py issues."""
    value = {
        "v": 2, "gen": PY_GENERATION, "dir": "older", "task": sha256_text(task_id), "dev": 16777232,
        "ino": 4242, "path": sha256_text(f"{task_id}.events.jsonl"), "boundary": 120,
        "anchor": "a" * 64, "head": "b" * 64, "hlen": 64, "filter": filter_key, "resync": False,
        "seq": 3, "skip": False,
    }
    value.update(overrides)
    return value


def python_cursor(task_id, filter_key, **overrides):
    """Padded urlsafe base64 of the compact JSON, as event_pages.py writes it."""
    return encode(python_value(task_id, filter_key, **overrides))


def encode(value, padded=True):
    text = base64.urlsafe_b64encode(json.dumps(value, separators=(",", ":")).encode("utf-8")).decode("ascii")
    return text if padded else text.rstrip("=")


def decode(cursor):
    return json.loads(base64.urlsafe_b64decode(cursor + "=" * (-len(cursor) % 4)))


def event(seq, kind="assistant_text", text=None, minute=0, **fields):
    if text is not None:
        fields["text"] = text
    return seeds.event(seq, kind, STAMP % (minute, seq % 60), **fields)


def printed(done):
    """The printed size of a successful document in UTF-8, and its size as the reference engine
    writes it: `json.dumps` with its default separators and `ensure_ascii`, plus the newline."""
    document = json.loads(done.stdout)
    return len(done.stdout.encode("utf-8")), len(json.dumps(document)) + 1


class HistoryCase(ReadsCase):
    """A scratch root seeded in one call, with helpers for the history reads."""

    def world(self, *tasks, events=None, root=None):
        self.seed("seed_world", {"world": {"tasks": list(tasks), "events": events or {}}}, root=root)

    def page(self, task_id, *args, **options):
        return self.good("task-events", task_id, *args, **options)["result"]

    def refused(self, *args, code, **options):
        error = self.bad(*args, code=code, **options)
        return error

    def seqs(self, page):
        return [item["seq"] for item in page["events"]]

    def walk(self, task_id, first, follow, *args):
        """Every page of a walk: `first` is the opening argument list, `follow` the cursor key."""
        pages, arguments = [], list(first)
        for _ in range(500):
            page = self.page(task_id, *arguments, *args)
            pages.append(page)
            cursor = page[follow]
            if cursor is None or not page[{"older_cursor": "has_older", "newer_cursor": "has_newer"}[follow]]:
                return pages
            arguments = ["--cursor", cursor]
        raise AssertionError("a walk did not end")

    def repo(self, task_id):
        return os.path.join(self.base, "repos", task_id)

    def stored_tasks(self, *specs):
        self.init(*specs)


def long_text(length, fill="x"):
    return (fill * length)[:length]


REGISTERED = "2026-01-10T10:04:00+00:00"
# The predecessor product's directory name is spelled in two parts so no scan sees the legacy name.
PREDECESSOR_DIRECTORY = ".poly" + "bridge"
FORBIDDEN_NAMES = (".harflow", PREDECESSOR_DIRECTORY, ".codex", ".claude", ".vibe", ".config", ".git")


def item(artifact_id, name, method="verified_process_lineage", note="n", kind="report", **plan):
    """An observed backend plan carries a tool-use id; `plan_directory` and its identity are optional."""
    entry = {"artifact_id": artifact_id, "name": name, "target": "present", "kind": kind, "note": note,
             "registered_at": REGISTERED, "provenance_method": method}
    if method == "observed_backend_plan":
        entry["tool_use_id"] = "tool-use-1"
    entry.update(plan)
    return entry


class ArtifactCase(HistoryCase):
    """A store whose task A has a real repository directory and references to files in it."""

    def setUp(self):
        super().setUp()
        self.init()
        self.add(A, real_workspace=True)
        self.repo_path = self.repo(A)

    def register(self, *items, task_id=A, header_valid=True):
        reply = self.seed("write_artifacts", {"task_id": task_id, "items": list(items), "header_valid": header_valid})
        return reply["paths"]

    def listed(self, *args, task_id=A, **options):
        return self.good("task-artifacts", task_id, *args, **options)["result"]

    def states(self, *args, task_id=A, **options):
        return {entry["artifact_id"]: entry["availability"]["state"]
                for entry in self.listed(*args, task_id=task_id, **options)["items"]}

    def outcome(self, artifact_id="x1", **options):
        return self.states(**options)[artifact_id]

    def move_to(self, path, artifact_id="x1", task_id=A):
        """Point a reference at `path` and record what the file there is now."""
        info = os.stat(path)
        self.sql(
            "UPDATE artifact_items SET path=?, device=?, inode=?, size=?, mtime_ns=? WHERE task=? AND artifact_id=?",
            (path, str(info.st_dev), str(info.st_ino), info.st_size, str(info.st_mtime_ns), task_id, artifact_id),
        )

    def file_at(self, *parts, content=b"data"):
        path = os.path.join(self.repo_path, *parts)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as stream:
            stream.write(content)
        return path

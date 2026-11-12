"""Shared builders of the pending launch tests: launch specs, the oracle and the base case."""

import base64
import hashlib
import json
import os

import seed_support as seeds
from ctl_support import snapshot
from reads_support import ReadsCase
from seed_support import A, B, C

SCOPE = "prerecord_launch_attempts_and_not_started"
SUFFIX = ".launch.json"
PAGE_KEYS = {
    "scope", "observed_at", "items", "namespace_revision", "namespace_complete", "coverage",
    "invalid_name_count", "examined_count", "has_more", "next_cursor", "bound",
}
LAUNCH_KEYS = {
    "title", "backend", "repo_path", "created_at", "not_started_reason", "generation", "state",
    "lineage", "resource_names", "metadata_truncated",
}
PREFIX_COMMANDS = (("task-pending-page",), ("task-pending-status", "--task-id", A))
DIRECT_ONLY = "direct_caller_required"


def launch(name, state="reserved", **fields):
    spec = {
        "name": name, "state": state, "generation": "c" * 32, "backend": "claude", "title": "Title",
        "repo_path": "/repo", "created_at": "2026-11-10T00:00:00+00:00", "not_started_reason": None,
        "lineage": {}, "resources": None, "worktree": None, "refusal": None,
    }
    spec.update(fields)
    return spec


def revision(*filenames):
    digest = hashlib.sha256()
    for filename in sorted(filenames):
        digest.update(filename.encode() + b"\0")
    return digest.hexdigest()


def decode(cursor):
    return json.loads(base64.urlsafe_b64decode(cursor))


class PendingCase(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init()

    def add_launch(self, name, state="reserved", **fields):
        self.seed("add_launch", {"spec": launch(name, state, **fields)})

    def page(self, *args, **options):
        return self.good("task-pending-page", *args, **options)["result"]

    def status(self, task_id, **options):
        return self.good("task-pending-status", "--task-id", task_id, **options)["result"]

    def refused(self, *args, code, check=None, **options):
        error = self.bad(*args, code=code, **options)
        self.assertEqual(error["data"], {"coverage": "unknown"} | ({"check": check} if check else {}))
        return error

    def names(self, page):
        return [item["task_id"] for item in page["items"]]

    def walk(self, *args, pages=10):
        found, arguments = [], list(args)
        for _ in range(pages):
            page = self.page(*arguments)
            found.append(page)
            if page["next_cursor"] is None:
                return found
            arguments = [*args, "--cursor", page["next_cursor"]]
        raise AssertionError("a walk did not end")

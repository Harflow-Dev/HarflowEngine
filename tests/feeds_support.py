"""Shared helpers for the black-box tests of `recovery-activity`, `interaction-events` and
`statistics-page`.

The oracle is Python's own json, base64, datetime, hashlib and sqlite3: nothing is shared with the
Rust codecs. Literals that stand for the reference engine (HarflowMCP commit a426c5a) were derived
once with python3 3.13.5, either by importing its modules from `git show` output or with the
stdlib calls named beside each literal.
"""

import base64
import json
import os
import sqlite3
from datetime import datetime, timedelta, timezone

import seed_support as seeds
from reads_support import ReadsCase
from seed_support import A, B, C, D, E

WIDE = ("2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z")
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
EMPTY_GENERATION = "20fd013481cc79458c65358246d976f939e27afcf80cf6bb4996b48070ae21d3"
SOURCES_PER_PAGE = 25
ITEM_KEYS = 35
IE_DIAG = "Invalid interaction-events cursor"
IE_LIMIT = "limit must be between 1 and 100"
STATS_PARAMS = "Statistics require ordered timezone-qualified bounds and limit 1..100"
STATS_DECODE = "Invalid statistics cursor"
STATS_FOREIGN = "Statistics cursor belongs to another query or state root"
STATS_ORDER = "Statistics cursor order is invalid"
STATS_EXPIRED = "Retained statistics history changed; restart the page walk"
STATS_COVERAGE = "Statistics source coverage changed; restart the page walk"


def micros(text):
    """Microseconds since the epoch of an ISO instant with an offset, by exact stdlib arithmetic."""
    return (datetime.fromisoformat(text.replace("Z", "+00:00")) - EPOCH) // timedelta(microseconds=1)


def stamp(minute, day=10, second=0, year=2026):
    return "%04d-01-%02dT10:%02d:%02d+00:00" % (year, day, minute, second)


def settled(task_id, start=0, end=None, day=10, **fields):
    """A finished task: it starts at minute `start` of 10:00 on `day` and ends at minute `end`."""
    end = start + 5 if end is None else end
    return seeds.task(task_id, started_at=stamp(start, day), finished_at=stamp(end, day), **fields)


def decode_cursor(text):
    return json.loads(base64.urlsafe_b64decode(text + "=" * (-len(text) % 4)))


def stats_cursor(root, window, generation, offset):
    """A statistics cursor in the shape the engine documents: integer microsecond bounds."""
    value = {"v": 1, "root": root, "since": micros(window[0]), "before": micros(window[1]),
             "generation": generation, "offset": offset}
    return base64.urlsafe_b64encode(json.dumps(value, separators=(",", ":")).encode()).decode()


def ascii_size(document, compact=False):
    """The bytes of json.dumps with ensure_ascii, the reference engine's default."""
    separators = (",", ":") if compact else None
    return len(json.dumps(document, separators=separators).encode("utf-8"))


class FeedsCase(ReadsCase):
    """A scratch root and helpers for the three reads."""

    def world(self, *tasks, root=None):
        self.seed("seed_world", {"world": {"tasks": list(tasks), "events": {}}}, root=root)

    def feed(self, *args, **options):
        return self.good("interaction-events", *args, **options)["result"]

    def feed_error(self, *args, code, **options):
        return self.bad("interaction-events", *args, code=code, **options)

    def stats(self, *args, window=WIDE, **options):
        return self.good("statistics-page", "--since", window[0], "--before", window[1], *args, **options)["result"]

    def stats_error(self, *args, code, window=WIDE, **options):
        return self.bad("statistics-page", "--since", window[0], "--before", window[1], *args, code=code, **options)

    def recovery(self, *args, **options):
        return self.good("recovery-activity", *args, **options)

    def feed_walk(self, *args, pages=40):
        found = []
        arguments = list(args)
        for _ in range(pages):
            page = self.feed(*arguments)
            found.append(page)
            if page["next_cursor"] is None:
                return found
            arguments = [*args, "--cursor", page["next_cursor"]]
        raise AssertionError("a walk did not end")

    def stats_walk(self, *args, window=WIDE, pages=60):
        found = []
        arguments = list(args)
        for _ in range(pages):
            page = self.stats(*arguments, window=window)
            found.append(page)
            if page["next_cursor"] is None:
                return found
            arguments = [*args, "--cursor", page["next_cursor"]]
        raise AssertionError("a walk did not end")

    def reading(self, path, query):
        """Rows of the root's database, read through a private connection."""
        db = sqlite3.connect("file:%s?mode=ro" % os.path.join(self.state, "store.sqlite3"), uri=True)
        try:
            return db.execute(path, query).fetchall()
        finally:
            db.close()

    def facts(self, task_id, **columns):
        """Rewrite columns of a task's fact row and nothing else."""
        names = ",".join("%s=?" % name for name in columns)
        self.sql("UPDATE task_facts SET %s WHERE id=?" % names, (*columns.values(), task_id))

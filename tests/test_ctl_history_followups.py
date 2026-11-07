"""The recorded process in the change token, a bad stream row under `--brief`, and the artifact
page at its exact byte bound."""

import json

import seed_support as seeds
from history_support import ArtifactCase, HistoryCase, event, item
from seed_support import A, B

LIVE = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f"
PAGE_BYTES = 63_000


class ProcessTokenTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))
        self.live(LIVE, identity="reused", status="completed_observed")
        for n in range(3):
            self.seed("append_event", {"task_id": LIVE, **event(n, "notice", f"n{n}")})

    def token(self):
        return self.page(LIVE)["change_token"]

    def test_a_change_of_the_recorded_pid_changes_the_token(self):
        before = self.token()
        self.assertEqual(self.token(), before)
        self.sql("UPDATE task_processes SET pid=pid+1 WHERE task=?", (LIVE,))
        self.assertNotEqual(self.token(), before)

    def test_a_change_of_the_recorded_start_identity_changes_the_token(self):
        for column in ("start_micros", "start_seconds", "boot_uuid"):
            with self.subTest(column=column):
                before = self.token()
                value = "'00000000-0000-4000-8000-000000000000'" if column == "boot_uuid" else f"{column}+1"
                self.sql(f"UPDATE task_processes SET {column}={value} WHERE task=?", (LIVE,))
                self.assertNotEqual(self.token(), before)


class BadStreamRowTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def test_a_bad_stream_row_gives_an_empty_summary_and_a_successful_status(self):
        before = self.good("status", A, "--brief")["task"]["recent_activity"]
        self.assertTrue(before)
        for statement in (
            "UPDATE event_streams SET generation='not hex' WHERE task=?",
            "UPDATE event_streams SET capture=9 WHERE task=?",
            "UPDATE event_streams SET complete_end=-5 WHERE task=?",
        ):
            with self.subTest(statement=statement):
                self.setUp()
                self.sql(statement, (A,))
                task = self.good("status", A, "--brief")["task"]
                self.assertEqual(task["recent_activity"], [])
                self.assertEqual(task["status"], "completed")
                error = self.bad("task-events", A, code="state_incompatible")
                self.assertEqual(error["data"]["reason"], "infrastructure_unavailable")

    def test_a_damaged_row_of_one_task_does_not_blank_another(self):
        self.sql("UPDATE event_streams SET generation='not hex' WHERE task=?", (A,))
        self.assertTrue(self.good("status", B, "--brief")["task"]["recent_activity"])

    def test_the_plain_status_never_reads_the_log(self):
        self.sql("UPDATE event_streams SET generation='not hex' WHERE task=?", (A,))
        self.assertNotIn("recent_activity", self.good("status", A)["task"])


class PageBoundTests(ArtifactCase):
    """The candidate page is measured as the reference measures it: the resolved task, the items,
    no cursor, `has_more` false, `count` 0, sized with default separators and `ensure_ascii`."""

    def candidate(self, result):
        return len(json.dumps({
            "task_id": result["task_id"], "items": result["items"], "next_cursor": None,
            "has_more": False, "state": "available", "count": 0,
        }))

    def setUp(self):
        super().setUp()
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt", note="n" * 1000) for n in range(50)))

    def test_a_page_of_exactly_the_bound_is_whole_and_one_byte_more_drops_the_last_entry(self):
        first = self.listed("--limit", "50")
        count = len(first["items"])
        self.assertTrue(0 < count < 50)
        slack = PAGE_BYTES - self.candidate(first)
        last = f"a{count - 1:02d}"
        self.sql("UPDATE artifact_items SET note=note||? WHERE task=? AND artifact_id=?", ("n" * slack, A, last))
        exact = self.listed("--limit", "50")
        self.assertEqual((len(exact["items"]), self.candidate(exact)), (count, PAGE_BYTES))
        self.sql("UPDATE artifact_items SET note=note||'n' WHERE task=? AND artifact_id=?", (A, last))
        over = self.listed("--limit", "50")
        self.assertEqual(len(over["items"]), count - 1)
        self.assertLessEqual(self.candidate(over), PAGE_BYTES)

    def test_the_cursor_and_the_prefix_echo_are_not_counted(self):
        first = self.listed("--limit", "50")
        again = self.listed("--limit", "50", task_id=A[:8])
        self.assertEqual(len(again["items"]), len(first["items"]))
        self.assertIn("resolved_from", again)

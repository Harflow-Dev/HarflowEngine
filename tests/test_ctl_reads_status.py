"""Black-box tests of `status`, `status-batch`, `list` and the transport around them."""

import datetime
import json
import os
import subprocess
import time
import unittest

import seed_support as seeds
from ctl_support import binary
from reads_support import ENVELOPE, MISSING, ReadsCase, day
from seed_support import A, B, C, D

TASK_KEYS = {
    "artifact_summary", "backend", "chain_root_task_id", "continuation_of_task_id",
    "continuation_stop", "duration_seconds", "exit_code",
    "finished_at", "freedom", "group", "model", "needs_reconciliation", "observation_mode",
    "observed_exit", "output_drained", "owner", "persisted_status", "phase_slots",
    "process_identity_state", "recovered", "repo_path", "session_id", "started_at", "status",
    "task_id", "title",
}
NEVER_INVENTED = {
    "parent_task_id", "last_output_tail", "summary", "raw_stream_log", "events_log",
    "resume_command", "messages", "recovery_activity",
}


class TransportTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A))

    def test_a_json_flag_or_its_unique_prefix_selects_the_envelope(self):
        for flag in ("--json", "--js", "--j"):
            with self.subTest(flag=flag):
                done = self.read("list", flag)
                self.assertEqual((done.returncode, done.stderr), (0, ""))
                self.assertEqual(done.stdout.count("\n"), 1)
                self.assertTrue(ENVELOPE <= set(json.loads(done.stdout)))

    def test_other_flags_do_not_select_json(self):
        cases = (
            ("list", "--since", "10000d"), ("status", A, "--observer-parent"),
            ("status-batch", "--task-ids", A), ("task-list-page", "--active-only"),
            ("task-list-page", "--limit", "5"), ("task-list-page", "--session-id", "x"),
            ("task-list-page", "--cursor=--json"), ("status", A, "--observer-parent"),
        )
        for args in cases:
            with self.subTest(args=args):
                done = self.read(*args)
                if args[1:] == ("--cursor=--json",):
                    self.assertEqual((done.returncode, done.stdout), (1, ""))
                    continue
                self.assertEqual((done.returncode, done.stderr), (0, ""))
                self.assertGreater(done.stdout.count("\n"), 3, "pretty printed")
                self.assertFalse(ENVELOPE & set(json.loads(done.stdout)))

    def test_a_value_spelled_json_after_the_terminator_is_not_the_flag(self):
        done = self.read("status", "--", "--json")
        self.assertEqual((done.returncode, done.stdout), (1, ""))
        self.assertEqual(done.stderr.count("\n"), 1)

    def test_runtime_error_exit_and_streams(self):
        plain = self.read("status", MISSING)
        self.assertEqual((plain.returncode, plain.stdout), (1, ""))
        self.assertTrue(plain.stderr.startswith("harflow-ctl: error: unknown task_id"))
        self.assertEqual(plain.stderr.count("\n"), 1)
        for flag in ("--json", "--js"):
            done = self.read("status", MISSING, flag)
            self.assertEqual(done.returncode, 1)
            self.assertEqual(done.stderr, plain.stderr)
            self.assertEqual(done.stdout.count("\n"), 1)
            self.assertEqual(json.loads(done.stdout)["error"]["code"], "unknown_task")

    def test_usage_errors_are_exit_two(self):
        cases = (
            ("task-list-page", "--limit", "abc"), ("task-list-page", "--limit", "1.5"),
            ("task-list-page", "--limit"), ("task-list-page", "--bogus"), ("status",),
            ("status", A, B), ("status-batch",), ("list", "--since", "bogus"),
            ("list", "--since", "-3d"), ("list", "extra"),
        )
        for args in cases:
            with self.subTest(args=args):
                done = self.read(*args)
                self.assertEqual((done.returncode, done.stdout), (2, ""))
                self.assertTrue(done.stderr.startswith("harflow-ctl"), done.stderr)
                loud = self.read(*args, "--json")
                self.assertEqual(loud.returncode, 2)
                self.assertEqual(json.loads(loud.stdout)["error"]["code"], "usage")

    def test_limit_outside_one_to_a_hundred_is_a_runtime_error(self):
        for value in ("0", "-1", "101"):
            with self.subTest(value=value):
                self.bad("task-list-page", "--limit", value, code="invalid_params")
        for value in ("1", "100", "+7", "1_0"):
            with self.subTest(value=value):
                self.good("task-list-page", "--limit", value)

    def test_option_forms_after_the_command(self):
        self.assertEqual(self.good("task-list-page", "--limit=1")["result"]["items"][0]["task_id"], A)
        self.assertEqual(self.good("task-list-page", "--lim", "1")["result"]["has_more"], False)
        done = self.read("status", "--json", "--", A)
        self.assertEqual(json.loads(done.stdout)["task"]["task_id"], A)
        self.assertEqual(self.ids(self.good("list", "--since=10000d")["tasks"]), [A])


class StatusTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(
            seeds.task(A, title="First", group="g1", model="m1"),
            seeds.task(B, started_at=day(11), finished_at=day(11), status="failed", exit_code=1),
            seeds.task(C, started_at=day(12), finished_at=day(12), continuation_of=A),
        )

    def test_status_document_has_the_documented_keys_and_no_invented_ones(self):
        task = self.good("status", A)["task"]
        self.assertEqual(set(task), TASK_KEYS)
        self.assertFalse(NEVER_INVENTED & set(task))
        self.assertEqual(
            {key: task[key] for key in ("status", "persisted_status", "exit_code", "observed_exit",
                                        "process_identity_state", "needs_reconciliation",
                                        "duration_seconds", "title", "group", "model")},
            {"status": "completed", "persisted_status": "completed", "exit_code": 0,
             "observed_exit": True, "process_identity_state": "dead", "needs_reconciliation": False,
             "duration_seconds": 300.0, "title": "First", "group": "g1", "model": "m1"},
        )
        self.assertNotIn("resolved_from", task)

    def test_links_and_failed_status(self):
        task = self.good("status", C)["task"]
        self.assertEqual((task["continuation_of_task_id"], task["chain_root_task_id"]), (A, A))
        failed = self.good("status", B)["task"]
        self.assertEqual((failed["status"], failed["exit_code"]), ("failed", 1))

    def test_prefix_resolution_and_echo(self):
        task = self.good("status", A[:8])["task"]
        self.assertEqual(task["task_id"], A)
        self.assertEqual(task["resolved_from"], {"input": A[:8], "task_id": A})

    def test_duration_is_finished_minus_started_in_seconds(self):
        self.add(D, started_at="2026-01-20T10:00:00.123456+00:00",
                 finished_at="2026-01-20T10:00:01.000000+00:00")
        self.assertEqual(self.good("status", D)["task"]["duration_seconds"], 0.877)

    def test_an_unfinished_task_is_running_and_uncertain(self):
        self.add(D, status="running", exit_code=None, finished_at=None, output_drained=False)
        task = self.good("status", D)["task"]
        self.assertEqual(
            (task["status"], task["persisted_status"], task["process_identity_state"],
             task["needs_reconciliation"], task["observed_exit"]),
            ("running", "running", "uncertain", True, False),
        )

    def test_an_unfinished_task_has_a_duration_from_its_seeded_start_to_the_clock(self):
        started = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=1)
        self.add(D, status="running", exit_code=None, finished_at=None, output_drained=False,
                 started_at=started.isoformat())
        duration = self.good("status", D)["task"]["duration_seconds"]
        # An hour from the seeded start, plus the time this test took: generous on purpose.
        self.assertGreaterEqual(duration, 3600.0)
        self.assertLess(duration, 3600.0 + 300.0)

    def test_a_dead_unobserved_task_carries_the_reference_note(self):
        child = self.sleeper()
        self.live(D, pid=child.pid, identity="reused")
        task = self.good("status", D)["task"]
        self.assertEqual(task["status"], "unknown")
        self.assertEqual(task["note"], "process ended; outcome needs inspection")

    def test_a_json_prefix_is_honoured_on_usage_errors(self):
        done = self.read("list", "--js", "--since", "bogus")
        self.assertEqual(done.returncode, 2)
        self.assertEqual(json.loads(done.stdout)["error"]["code"], "usage")

    def test_page_items_carry_bounded_headers(self):
        self.add(D, title="t" * 700, started_at=day(20), finished_at=day(20))
        item = self.good("task-list-page", "--task-ids", D)["result"]["items"][0]
        self.assertLessEqual(len(item["title"]), 200)
        self.assertEqual(len(self.good("status", D)["task"]["title"]), 700)

    def test_error_documents(self):
        error = self.bad("status", MISSING, code="unknown_task")
        self.assertEqual(error["data"], {"code": "unknown_task", "input": MISSING})
        error = self.bad("status", "zz", code="unknown_task")
        self.assertEqual(error["data"]["reason"], "prefix_too_short")
        self.assertEqual(error["data"]["min_prefix_length"], 8)
        error = self.bad("status", "nomatch0123", code="unknown_task")
        self.assertEqual(error["data"]["reason"], "no_prefix_match")
        self.bad("status", "bad/id", code="invalid_task_id")

    def test_ambiguous_prefix_lists_candidates(self):
        for n in (1, 2):
            self.add("abababab-%04d-4000-8000-000000000001" % n, title="t%d" % n,
                     started_at=day(20 + n), finished_at=day(20 + n))
        error = self.bad("status", "abababab", code="ambiguous_task_id")
        data = error["data"]
        self.assertEqual((data["code"], data["input"], data["truncated"]),
                         ("ambiguous_task_id", "abababab", False))
        self.assertEqual(len(data["candidates"]), 2)
        for candidate in data["candidates"]:
            self.assertEqual(set(candidate), {"task_id", "title", "status", "created_at"})
        by_id = {c["task_id"]: c for c in data["candidates"]}
        first = by_id["abababab-0001-4000-8000-000000000001"]
        self.assertEqual((first["title"], first["status"], first["created_at"]),
                         ("t1", "completed", day(21)))

    def test_more_than_five_candidates_are_truncated(self):
        for n in range(1, 8):
            self.add("cdcdcdcd-%04d-4000-8000-000000000001" % n)
        data = self.bad("status", "cdcdcdcd", code="ambiguous_task_id")["data"]
        self.assertIs(data["truncated"], True)
        self.assertEqual(len(data["candidates"]), 5)
        self.assertIn("more than 5", self.bad("status", "cdcdcdcd")["message"])

    def test_observer_parent_fields(self):
        task = self.good("status", A, "--observer-parent")["task"]
        self.assertEqual(task["observer_kind"], "parent_process")
        self.assertEqual(task["observer_state"], "alive")
        self.assertEqual(set(task["observing_process"]), {"pid", "start_time"})
        self.assertEqual(task["observing_process"]["pid"], os.getpid())
        self.assertNotIn("observation_note", task)
        started = datetime.datetime.fromisoformat(task["observing_process"]["start_time"])
        self.assertEqual(started.utcoffset(), datetime.timedelta(0))
        text = subprocess.run(["ps", "-o", "lstart=", "-p", str(os.getpid())], capture_output=True,
                              text=True, env={"LC_ALL": "C", "PATH": "/bin"}).stdout.strip()
        local = datetime.datetime.strptime(" ".join(text.split()), "%a %b %d %H:%M:%S %Y")
        self.assertAlmostEqual(started.timestamp(), time.mktime(local.timetuple()), delta=1.0)
        self.assertNotIn("observer_kind", self.good("status", A)["task"])

    def test_observer_parent_is_undecidable_when_the_parent_is_gone(self):
        # An orphan has no qualified parent: the shell exits at once and init adopts the command.
        out = os.path.join(self.base, "orphan.json")
        script = '( sleep 1; exec "$0" "$@" > "%s" ) &' % out
        environment = {k: v for k, v in os.environ.items() if not k.startswith(("HARFLOW", "PB_"))}
        environment["HARFLOW_STATE_ROOT"] = self.state
        subprocess.run(["/bin/sh", "-c", script, str(binary()), "status", A, "--observer-parent",
                        "--json"], env=environment, check=True, timeout=30)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline and not (os.path.exists(out) and os.path.getsize(out)):
            time.sleep(0.2)
        with open(out, encoding="utf-8") as handle:
            task = json.loads(handle.read())["task"]
        self.assertEqual(
            (task["observer_kind"], task["observing_process"], task["observer_state"]),
            ("parent_process", None, "undecidable"),
        )
        self.assertTrue(task["observation_note"])


class BatchTests(ReadsCase):
    def setUp(self):
        super().setUp()
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)))

    def test_rows_errors_dedupe_and_order(self):
        argument = ",".join([A, MISSING, "bad/id", B[:8], B, A])
        document = self.good("status-batch", "--task-ids", argument)
        self.assertEqual(self.ids(document["tasks"]), [A, B, B])
        self.assertEqual(document["tasks"][1]["resolved_from"], {"input": B[:8], "task_id": B})
        self.assertNotIn("resolved_from", document["tasks"][2])
        self.assertEqual(sorted(document["errors"]), sorted([MISSING, "bad/id"]))
        self.assertEqual(document["errors"][MISSING]["code"], "unknown_task")
        self.assertEqual(document["errors"]["bad/id"]["code"], "invalid_task_id")
        self.assertEqual(set(document), ENVELOPE | {"tasks", "errors"})

    def test_a_repeated_id_is_one_row(self):
        document = self.good("status-batch", "--task-ids", f"{A},{A},{A}")
        self.assertEqual((self.ids(document["tasks"]), document["errors"]), ([A], {}))

    def test_bounds(self):
        many = ",".join(f"x{n}" for n in range(101))
        self.bad("status-batch", "--task-ids", many, code="invalid_params")
        hundred = ",".join([A] + [f"x{n}" for n in range(99)])
        document = self.good("status-batch", "--task-ids", hundred)
        self.assertEqual((len(document["tasks"]), len(document["errors"])), (1, 99))
        repeated = ",".join([A] * 150)
        self.assertEqual(len(self.good("status-batch", "--task-ids", repeated)["tasks"]), 1)

    def test_ambiguity_and_short_prefix_rows(self):
        for n in range(1, 8):
            self.add("cdcdcdcd-%04d-4000-8000-000000000001" % n)
        document = self.good("status-batch", "--task-ids", "cdcdcdcd,zz")
        ambiguous = document["errors"]["cdcdcdcd"]
        self.assertEqual(ambiguous["code"], "ambiguous_task_id")
        self.assertIs(ambiguous["data"]["truncated"], True)
        self.assertEqual(len(ambiguous["data"]["candidates"]), 5)
        self.assertEqual(document["errors"]["zz"]["data"]["reason"], "prefix_too_short")

    def test_observer_parent_is_stamped_on_every_row_once(self):
        document = self.good("status-batch", "--task-ids", f"{A},{B}", "--observer-parent")
        for task in document["tasks"]:
            self.assertEqual(task["observer_kind"], "parent_process")
            self.assertEqual(task["observing_process"]["pid"], os.getpid())
            self.assertEqual(task["observer_state"], "alive")


class ListTests(ReadsCase):
    def test_lists_every_task_newest_first_with_no_cap(self):
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)),
                  seeds.task(C, started_at=day(12), finished_at=day(12)))
        document = self.good("list")
        self.assertEqual(self.ids(document["tasks"]), [C, B, A])
        self.assertIs(document["history_incomplete"], False)
        self.assertEqual(document["refused_tasks"], [])

    def test_reads_past_one_hundred_rows(self):
        self.init()
        wanted = []
        for n in range(130):
            ident = "00000000-0000-4000-8000-%012d" % n
            stamp = "2026-02-01T10:%02d:%02d+00:00" % (n // 60, n % 60)
            self.add(ident, started_at=stamp, finished_at=stamp)
            wanted.append(ident)
        document = self.good("list")
        self.assertEqual(self.ids(document["tasks"]), wanted[::-1])
        self.assertIs(document["history_incomplete"], False)

    def test_since_keeps_recent_starts(self):
        recent = (datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=1))
        stamp = recent.isoformat()
        self.init(seeds.task(A), seeds.task(D, started_at=stamp, finished_at=stamp))
        self.assertEqual(self.ids(self.good("list", "--since", "1d")["tasks"]), [D])
        self.assertEqual(self.ids(self.good("list", "--since", "7d")["tasks"]), [D])
        self.assertEqual(self.ids(self.good("list", "--since", "10000d")["tasks"]), [D, A])
        self.assertEqual(self.ids(self.good("list", "--since", "0s")["tasks"]), [])
        self.assertEqual(self.ids(self.good("list", "--since", "2h")["tasks"]), [D])

    def test_a_header_near_the_stored_blob_limit_stays_readable(self):
        self.init(seeds.task(A, title="t" * 700, repo_path="/" + "r" * 700))
        self.assertEqual(self.ids(self.good("list")["tasks"]), [A])
        self.assertEqual(self.good("status", A)["task"]["title"], "t" * 700)

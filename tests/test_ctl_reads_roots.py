"""Black-box tests of root classification, read side effects and reads racing a writer."""

import json
import os
import subprocess

import seed_support as seeds
from ctl_support import rust_root, snapshot, storage_constants
from ctl_table import ROWS
from reads_support import ReadsCase, day
from seed_support import A, B, C, D

NO_STORE_ROWS = {
    "missing", "missing under a symlinked ancestor", "empty directory", "lock file only",
    "pending marker only", "legacy empty directories", "legacy empty directories and lock",
}
STATE_KEYS = {"engine_version", "reason", "recovery_supported", "state_root"}
EMPTY_PAGE = {
    "bootstrap_pending": False, "catalog_state": {
        "blocked_records": 0, "pending_records": 0, "source": "task_catalog", "status": "ready"},
    "counts_complete": True, "has_more": False, "history_incomplete": False, "items": [],
    "next_cursor": None, "related_headers": [], "total_active_count": 0,
    "total_active_root_count": 0, "total_attention_root_count": 0,
}


def listing(root):
    """The directory listing with sizes, without the shared-memory index a reader may touch."""
    return {key: value for key, value in snapshot(root).items() if not key.endswith("-shm")}


class NoStoreAnswerTests(ReadsCase):
    def roots(self):
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        return {"missing": os.path.join(self.base, "nothing"), "empty": empty}

    def test_every_read_answers_per_the_no_store_list_and_creates_nothing(self):
        for label, root in self.roots().items():
            before = listing(self.base)
            with self.subTest(root=label):
                self.assertEqual(self.good("list", root=root)["tasks"], [])
                self.assertEqual(self.good("list", "--since", "1d", root=root)["tasks"], [])
                error = self.bad("status", A, code="unknown_task", root=root)
                self.assertEqual(error["data"], {"code": "unknown_task", "input": A})
                self.bad("status", A[:8], code="unknown_task", root=root)
                batch = self.good("status-batch", "--task-ids", f"{A},bad/id,{A[:8]},zz", root=root)
                self.assertEqual(batch["tasks"], [])
                self.assertEqual(
                    {key: value["code"] for key, value in batch["errors"].items()},
                    {A: "unknown_task", "bad/id": "invalid_task_id", A[:8]: "unknown_task", "zz": "unknown_task"},
                )
                page = self.good("task-list-page", root=root)["result"]
                self.assertEqual(page, EMPTY_PAGE)
                self.assertEqual(self.good("task-list-page", "--limit", "1", "--active-only",
                                           "--session-id", "x", root=root)["result"], EMPTY_PAGE)
                self.bad("task-list-page", "--predecessor-task-id", A, code="unknown_task", root=root)
                ids = self.good("task-list-page", "--task-ids", f"{A},{B}", root=root)["result"]
                self.assertEqual((ids["items"], ids["related_headers"], ids["has_more"]), ([], [], False))
                self.bad("task-list-page", "--task-ids", f"{A},not-a-real-one-1", code="unknown_task", root=root)
                self.bad("task-list-page", "--task-ids", "bad/id", code="invalid_params", root=root)
                self.bad("task-list-page", "--limit", "0", code="invalid_params", root=root)
            self.assertEqual(listing(self.base), before)

    def test_a_missing_root_stays_missing(self):
        root = self.roots()["missing"]
        for args in (("list",), ("status", A), ("task-list-page",)):
            self.read(*args, "--json", root=root)
        self.assertFalse(os.path.lexists(root))


class RefusedRootTests(ReadsCase):
    def test_every_refused_root_of_the_inspection_table_refuses_reads_with_its_reason(self):
        for name, build, status, reason in ROWS:
            if status in ("ready", "missing"):
                continue
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    before = snapshot(self.base)
                    for args in (("list",), ("status", A), ("task-list-page",), ("status-batch", "--task-ids", A)):
                        error = self.bad(*args, code="state_incompatible", root=root, home=self.home)
                        self.assertEqual(set(error["data"]), STATE_KEYS)
                        self.assertEqual(error["data"]["reason"], reason, args)
                        self.assertEqual(error["data"]["state_root"], root)
                        self.assertIs(error["data"]["recovery_supported"], False)
                    self.assertEqual(snapshot(self.base), before)
                finally:
                    self.cleanup()

    def test_the_ready_rows_without_a_store_answer_as_empty(self):
        for name, build, status, _ in ROWS:
            if name not in NO_STORE_ROWS:
                continue
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    before = snapshot(self.base)
                    self.assertEqual(self.good("list", root=root)["tasks"], [])
                    self.bad("status", A, code="unknown_task", root=root)
                    self.assertEqual(snapshot(self.base), before)
                finally:
                    self.cleanup()

    def test_a_foreign_root_carries_the_state_data(self):
        self.seed("make_foreign_root")
        error = self.bad("list", code="state_incompatible")
        self.assertEqual(error["data"]["reason"], "unmarked_nonempty")
        self.assertEqual(error["data"]["state_root"], self.state)
        self.assertEqual(self.seed("root_state"), "other")

    def test_future_and_older_schema_roots(self):
        current = storage_constants()
        for label, version, reason in (("future", current + 1, "newer_store"), ("older", current - 1, "unsupported_layout")):
            with self.subTest(schema=label):
                self.setUp()
                rust_root(self.state, identity_version=version, header_version=version)
                for args in (("list",), ("status", A), ("task-list-page",), ("task-list-page", "--task-ids", A)):
                    error = self.bad(*args, code="state_incompatible")
                    self.assertEqual(error["data"]["reason"], reason)

    def test_a_database_with_a_future_header_only(self):
        current = storage_constants()
        rust_root(self.state, identity_version=current, header_version=current + 1)
        self.assertEqual(self.bad("list", code="state_incompatible")["data"]["reason"], "newer_store")

    def test_a_marked_store_without_a_write_ahead_log_is_incompatible(self):
        self.init(seeds.task(A))
        os.remove(os.path.join(self.state, "store.sqlite3-wal"))
        error = self.bad("list", code="state_incompatible")
        self.assertEqual(error["data"]["reason"], "infrastructure_unavailable")
        self.assertFalse(os.path.exists(os.path.join(self.state, "store.sqlite3-wal")))


class NoSideEffectTests(ReadsCase):
    def test_no_read_changes_the_authoritative_digest_or_creates_a_file(self):
        self.init(*seeds.world()["tasks"])
        self.add(D[:8] + "-0000-4000-8000-000000000001", status="running", exit_code=None, finished_at=None,
                 started_at=day(27))
        sleeper = self.sleeper()
        self.live("11111111-0000-4000-8000-000000000001", pid=sleeper.pid)
        digest = self.digest()
        names = sorted(os.listdir(self.state))
        files = listing(self.state)
        families = (
            ("list",), ("list", "--since", "7d"), ("status", A), ("status", A[:8], "--observer-parent"),
            ("status-batch", "--task-ids", f"{A},{B},bad/id"), ("task-list-page",),
            ("task-list-page", "--limit", "1", "--active-only"), ("task-list-page", "--task-ids", f"{A},{C}"),
            ("task-list-page", "--predecessor-task-id", A), ("task-list-page", "--session-id", "x"),
        )
        for args in families:
            with self.subTest(args=args):
                self.read(*args, "--json")
                self.assertEqual(self.digest(), digest)
                self.assertEqual(sorted(os.listdir(self.state)), names)
                self.assertEqual(listing(self.state), files)

    def test_refused_reads_change_nothing_either(self):
        self.init(*seeds.world()["tasks"])
        digest = self.digest()
        files = listing(self.state)
        for args in (("list",), ("task-list-page", "--predecessor-task-id", A), ("task-list-page",)):
            self.bad(*args, env={"HARFLOW_TASK_ID": "x"})
        self.assertEqual(self.digest(), digest)
        self.assertEqual(listing(self.state), files)
        self.live("11111111-0000-4000-8000-000000000001")
        digest = self.digest()
        files = listing(self.state)
        for args in (("list",), ("status", A), ("task-list-page", "--predecessor-task-id", A)):
            self.bad(*args)
        self.assertEqual(self.digest(), digest)
        self.assertEqual(listing(self.state), files)


class RaceTests(ReadsCase):
    """A second process writing or damaging the root while the reads run."""

    ADDED = 12

    def writer(self):
        script = (
            "import json,subprocess,sys\n"
            "binary,root,damage,ids=sys.argv[1],sys.argv[2],sys.argv[3],json.loads(sys.argv[4])\n"
            "def call(op,args):\n"
            "    subprocess.run([binary,op,'--root',root],input=json.dumps(args).encode(),check=True,\n"
            "        capture_output=True,env={'PATH':'/usr/bin:/bin'})\n"
            "for spec in ids:\n"
            "    call('add_task',{'spec':spec})\n"
            "call('damage_task',{'task_id':damage})\n"
        )
        specs = [seeds.task("%08d-0000-4000-8000-000000000001" % n, started_at=day(15 + n % 10),
                            finished_at=day(15 + n % 10)) for n in range(self.ADDED)]
        return subprocess.Popen(
            ["/usr/bin/python3", "-I", "-c", script, str(seeds.binary()), self.state, B, json.dumps(specs)],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )

    def test_reads_during_writes_and_damage_are_whole_or_refused(self):
        self.init(seeds.task(A), seeds.task(B, started_at=day(11), finished_at=day(11)),
                  seeds.task(C, started_at=day(12), finished_at=day(12)))
        base = {A, B, C}
        self.assertEqual(set(self.ids(self.good("list")["tasks"])), base)  # an answer before any write
        process = self.writer()
        self.addCleanup(self.reap, process)
        answers, damaged_seen, previous, overlap, largest = 0, False, 0, 0, 0
        while True:
            finished = process.poll() is not None
            for args in (("list",), ("task-list-page", "--limit", "100")):
                done = self.read(*args, "--json")
                document = json.loads(done.stdout)
                if done.returncode == 0:
                    body = document.get("tasks", document.get("result", {}).get("items"))
                    if args[0] == "task-list-page" and document["result"].get("authority_incomplete"):
                        self.assertEqual(body, [])
                        damaged_seen = True
                        continue
                    ids = self.ids(body)
                    self.assertEqual(len(ids), len(set(ids)), "duplicate rows")
                    self.assertTrue(base <= set(ids), "a read lost a task: %s" % ids)
                    if args[0] == "list":
                        self.assertGreaterEqual(len(ids), previous, "the history shrank")
                        previous = len(ids)
                    for task in body:
                        self.assertIn("process_identity_state", task)
                        self.assertIn("status", task)
                    answers += 1
                    largest = max(largest, len(ids))
                    overlap += 0 if finished else 1
                else:
                    code = document["error"]["code"]
                    self.assertIn(code, ("state_busy", "workflow_read_refused"), document)
                    if code != "state_busy":
                        damaged_seen = True
            if finished:
                break
        self.addCleanup(process.stderr.close)
        self.assertEqual(process.wait(timeout=120), 0, process.stderr.read())
        self.bad("list", code="workflow_read_refused")
        page = self.good("task-list-page")["result"]
        self.assertEqual((page["items"], page["history_incomplete"]), ([], True))
        self.assertGreater(answers, 0)
        self.assertGreater(overlap, 0, "no answer was read while the writer was running")
        self.assertGreater(largest, len(base), "no read saw a write land")

"""Black-box tests of the pending launch reads: the caller gate, the limit seam, ordinary reads
of a receipt and usage."""

from pending_support import *  # noqa: F401,F403
from pending_support import PendingCase


class GateTests(PendingCase):
    def test_a_marker_refuses_before_the_root_is_looked_at(self):
        self.add_launch(A)
        keys = ("HARFLOW_TASK_ID", "PB_TASK_ID", "HARFLOW_DEPTH", "PB_DEPTH")
        for command in PREFIX_COMMANDS:
            for key in keys:
                for value in ("x", ""):
                    for root in (self.state, os.path.join(self.base, "missing")):
                        with self.subTest(command=command, key=key, value=value):
                            error = self.refused(*command, code="agent_caller", check=DIRECT_ONLY,
                                                 env={key: value}, root=root)
                            self.assertEqual(error["message"], "Pending launch reads require a verified direct caller.")

    def test_the_task_root_marker_is_not_one_of_them(self):
        self.add_launch(A)
        self.assertEqual(len(self.page(env={"HARFLOW_ROOT_TASK_ID": "x"})["items"]), 1)

    def test_a_live_agent_ancestor_is_an_agent_caller(self):
        self.add_launch(A)
        self.live(B)
        for command in PREFIX_COMMANDS:
            self.refused(*command, code="agent_caller", check=DIRECT_ONLY)

    def test_an_identity_that_cannot_be_decided_is_caller_undecidable(self):
        self.add_launch(A)
        self.live(B, identity="unknown")
        for command in PREFIX_COMMANDS:
            self.refused(*command, code="caller_undecidable", check=DIRECT_ONLY)

    def test_a_damaged_record_anywhere_makes_the_caller_undecidable_and_discloses_nothing(self):
        self.add(B)
        self.add_launch(A, title="SECRET")
        self.seed("damage_task", {"task_id": C})
        for command in PREFIX_COMMANDS:
            error = self.refused(*command, code="caller_undecidable", check=DIRECT_ONLY)
            self.assertNotIn("SECRET", json.dumps(error))

    def test_the_gate_comes_before_the_limit_and_the_cursor(self):
        self.add_launch(A)
        self.live(B)
        self.refused("task-pending-page", "--limit", "0", "--cursor", "x", code="agent_caller", check=DIRECT_ONLY)
        self.refused("task-pending-status", "--task-id", "bad id!", code="agent_caller", check=DIRECT_ONLY)

    def test_an_unreadable_root_is_unavailable_without_a_check_and_is_not_created(self):
        missing = os.path.join(self.base, "missing")
        empty = os.path.join(self.base, "empty")
        os.mkdir(empty)
        for root in (missing, empty):
            for command in PREFIX_COMMANDS:
                error = self.refused(*command, code="pending_observation_unavailable", root=root)
                self.assertEqual(error["message"],
                                 "Pending launch coverage is unknown; selected state is unavailable.")
        self.assertFalse(os.path.exists(missing))
        self.assertEqual(os.listdir(empty), [])

    def test_a_foreign_root_is_unavailable(self):
        foreign = os.path.join(self.base, "foreign")
        self.seed("make_foreign_root", root=foreign)
        self.refused("task-pending-page", code="pending_observation_unavailable", root=foreign)

    def test_reads_change_nothing(self):
        self.add_launch(A)
        self.add_launch(B, refusal={"check": "format"})
        digest, names, before = self.digest(), sorted(os.listdir(self.state)), snapshot(self.base)
        for args in (("task-pending-page",), ("task-pending-page", "--limit", "1"),
                     ("task-pending-page", "--cursor", "x"), ("task-pending-status", "--task-id", A)):
            self.read(*args, "--json")
            self.read(*args)
        self.assertEqual(self.digest(), digest)
        self.assertEqual(sorted(os.listdir(self.state)), names)
        after = snapshot(self.base)
        for key, value in before.items():
            self.assertEqual(after[key][:2] if key.endswith("-shm") else after[key],
                             value[:2] if key.endswith("-shm") else value, key)


class SeamTests(PendingCase):
    """The debug build's limit seam: the environment variables only a debug binary reads."""

    def test_the_namespace_bounds_are_reached_through_the_seam(self):
        self.add_launch(A)
        self.add_launch(B)
        self.assertEqual(len(self.page(env={"HARFLOW_PENDING_ENTRIES": "2"})["items"]), 2)
        # The caller check meets the bound first, as in the reference, and discloses nothing.
        self.refused("task-pending-page", code="caller_undecidable", check=DIRECT_ONLY,
                     env={"HARFLOW_PENDING_ENTRIES": "1"})
        exact = 2 * (len(A) + len(SUFFIX))
        self.assertEqual(len(self.page(env={"HARFLOW_PENDING_NAME_BYTES": str(exact)})["items"]), 2)
        self.refused("task-pending-page", code="caller_undecidable", check=DIRECT_ONLY,
                     env={"HARFLOW_PENDING_NAME_BYTES": str(exact - 1)})
        # The seam only lowers a bound: a larger value changes nothing.
        self.assertEqual(len(self.page(env={"HARFLOW_PENDING_ENTRIES": "99999999"})["items"]), 2)

    def test_the_read_budget_bounds_a_page_and_the_caller_check(self):
        self.add_launch(A)
        self.add_launch(B)
        row = 65537  # the reference charges this for every receipt it reads
        page = self.page(env={"HARFLOW_PENDING_READ_BYTES": str(row)})
        self.assertEqual((len(page["items"]), page["bound"], page["has_more"], page["coverage"]),
                         (1, "read_budget", True, "unknown"))
        self.assertIs(decode(page["next_cursor"])["uncertain"], True)
        self.refused("task-pending-page", code="pending_observation_bound", check="read_budget",
                     env={"HARFLOW_PENDING_READ_BYTES": str(row - 1)})
        self.assertEqual(self.page(env={"HARFLOW_PENDING_READ_BYTES": str(2 * row)})["bound"], None)

    def test_an_exhausted_budget_in_the_record_check_is_undecidable(self):
        self.add(B)
        self.add_launch(A, title="SECRET")
        error = self.refused("task-pending-page", code="caller_undecidable", check=DIRECT_ONLY,
                             env={"HARFLOW_PENDING_READ_BYTES": "10"})
        self.assertNotIn("SECRET", json.dumps(error))


class OrdinaryReadTests(PendingCase):
    def test_an_unqualified_receipt_is_a_document_error_not_an_unknown_task(self):
        self.add_launch(A, refusal={"check": "oversize"})
        want = "%s/tasks/%s.launch.json [oversize]: Task launch exceeds 65536 bytes" % (self.state, A)
        for command in (("status", A), ("task-events", A)):
            error = self.bad(*command, code="oversize")
            self.assertEqual(error["message"], want)
            self.assertNotIn("data", error)
        batch = self.good("status-batch", "--task-ids", A)
        self.assertEqual(batch["errors"][A], {"code": "oversize", "message": want})

    def test_reads_that_never_open_receipts_treat_an_unqualified_one_as_unknown(self):
        self.add(B)
        for check in ("oversize", "field_shape"):
            with self.subTest(check=check):
                self.add_launch(A, refusal={"check": check})
                page = self.good("task-list-page", "--task-ids", A + "," + B)["result"]
                self.assertEqual(self.ids(page["items"]), [B])
                error = self.bad("task-artifacts", A, code="artifact_error")
                self.assertEqual(error["message"], "Unknown task")
                error = self.bad("recovery-activity", A, code="unknown_task")
                self.assertEqual(error["message"], "unknown task_id: " + A)

    def test_a_qualified_pending_kind_receipt_still_reports_the_launch(self):
        self.add_launch(A, "waiting_for_resources", resources={"names": ["gpu"]})
        error = self.bad("status", A, code="task_pending_launch")
        self.assertEqual(error["data"], {"launch_state": "waiting_for_resources"})

    def test_an_empty_resource_name_list_still_reads_as_a_pending_launch(self):
        self.add_launch(A, "granted", resources={"names": []})
        self.bad("status", A, code="task_pending_launch")
        self.assertEqual(self.status(A)["check"], "resource_names")


class UsageTests(PendingCase):
    def test_option_forms_prefixes_and_equals(self):
        for name in (A, B):
            self.add_launch(name)
        for args in (("--limit", "1"), ("--limit=1",), ("--lim", "1"), ("--li=1",)):
            with self.subTest(args=args):
                self.assertEqual(self.names(self.page(*args)), [A])
        cursor = self.page("--limit", "1")["next_cursor"]
        for args in (("--limit=1", "--cursor=" + cursor), ("--li", "1", "--cu", cursor)):
            self.assertEqual(self.names(self.page(*args)), [B])
        for args in (("--task-id=" + A,), ("--task", A), ("--t=" + A,)):
            self.assertEqual(self.status_args(*args)["task_id"], A)

    def status_args(self, *args):
        return self.good("task-pending-status", *args)["result"]

    def test_usage_errors_exit_two_with_the_argparse_text(self):
        cases = (
            (("task-pending-status",), "harflow-ctl task-pending-status: error: the following arguments are required: --task-id"),
            (("task-pending-page", "--limit", "x"), "harflow-ctl task-pending-page: error: argument --limit: invalid int value: 'x'"),
            (("task-pending-page", "--limit"), "harflow-ctl task-pending-page: error: argument --limit: expected one argument"),
            (("task-pending-page", "--bogus"), "harflow-ctl: error: unrecognized arguments: --bogus"),
            (("task-pending-status", "--task-id", A, "extra"), "harflow-ctl: error: unrecognized arguments: extra"),
        )
        for args, text in cases:
            with self.subTest(args=args):
                done = self.read(*args)
                self.assertEqual(done.returncode, 2, done.stdout)
                self.assertEqual(done.stdout, "")
                self.assertEqual(done.stderr, text + "\n")

    def test_help_is_the_argparse_usage(self):
        page = self.read("task-pending-page", "--help")
        self.assertEqual((page.returncode, page.stderr), (0, ""))
        self.assertEqual(page.stdout, "usage: harflow-ctl task-pending-page [-h] [--json] [--limit LIMIT]\n"
                                      "                                     [--cursor CURSOR]\n")
        status = self.read("task-pending-status", "-h")
        self.assertEqual(status.stdout, "usage: harflow-ctl task-pending-status [-h] [--json] --task-id TASK_ID\n")

    def test_the_plain_form_prints_the_bare_indented_result(self):
        self.add_launch(A)
        done = self.read("task-pending-page")
        self.assertEqual(done.returncode, 0, done.stderr)
        result = json.loads(done.stdout)
        self.assertEqual(set(result), PAGE_KEYS)
        self.assertGreater(done.stdout.count("\n"), 5)
        enveloped = self.good("task-pending-page")
        self.assertEqual(enveloped["result"]["items"], result["items"])

"""Artifact cursor failures carry `data.reason` in the order of the reference (HarflowMCP 0ac7e3c,
`_get_page`): shape, then the binding to owner and manifest revision, then the range against the
current manifest. Other artifact errors carry no `data`. Also the plan provenance of an
`observed_backend_plan` reference, seeded through hf-fixture-seed and read back through harflow-ctl."""

import base64
import hashlib
import json
import subprocess

import seed_support as seeds
from history_support import ArtifactCase, item
from seed_support import A, B

STALE = "Invalid or stale artifact cursor"
EMPTY_REVISION = hashlib.sha256(b"").hexdigest()
OTHER_REVISION = "0" * 32
PLAN = "observed_backend_plan"
IDENTITY = {"device": 2**64 - 1, "inode": 2**63}


def cursor_of(value):
    return base64.urlsafe_b64encode(json.dumps(value).encode()).decode()


class ReasonTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt") for n in range(5)))
        self.revision = json.loads(base64.urlsafe_b64decode(self.listed("--limit", "2")["next_cursor"]))[1]

    def reason(self, cursor, task_id=A):
        error = self.bad("task-artifacts", task_id, "--cursor", cursor, code="artifact_error")
        self.assertEqual(error["message"], STALE)
        self.assertEqual(set(error), {"code", "message", "data"})
        self.assertEqual(set(error["data"]), {"reason"})
        return error["data"]["reason"]

    def test_shape_beats_binding(self):
        for label, value in (
            ("negative offset, stale revision", [A, OTHER_REVISION, -1]),
            ("negative offset, other task", [B, self.revision, -1]),
            ("negative offset, current", [A, self.revision, -1]),
            ("text offset, stale revision", [A, OTHER_REVISION, "1"]),
            ("float offset, stale revision", [A, OTHER_REVISION, 1.5]),
            ("short, stale revision", [A, OTHER_REVISION]),
            ("number task", [1, self.revision, 1]),
        ):
            with self.subTest(label):
                self.assertEqual(self.reason(cursor_of(value)), "cursor_invalid")
        for label, cursor in (("not base64 json", "garbage"), ("empty", ""), ("too long", cursor_of([A, self.revision, "p" * 600]))):
            with self.subTest(label):
                self.assertEqual(self.reason(cursor), "cursor_invalid")

    def test_binding_beats_range(self):
        for label, value in (
            ("stale revision, beyond the count", [A, OTHER_REVISION, 99]),
            ("stale revision, u64 maximum", [A, OTHER_REVISION, 2**64 - 1]),
            ("stale revision, above i64", [A, OTHER_REVISION, 2**63]),
            ("other task, beyond the count", [B, self.revision, 99]),
            ("other task, u64 maximum", [B, self.revision, 2**64 - 1]),
        ):
            with self.subTest(label):
                self.assertEqual(self.reason(cursor_of(value)), "cursor_stale")

    def test_an_integer_of_any_size_is_a_valid_offset(self):
        for offset in (2**64, 10**30):
            with self.subTest(offset=offset):
                self.assertEqual(self.reason(cursor_of([A, OTHER_REVISION, offset])), "cursor_stale")
                self.assertEqual(self.reason(cursor_of([A, self.revision, offset])), "cursor_invalid")

    def test_range_is_checked_against_the_current_manifest(self):
        for offset in (6, 99, 2**63 - 1, 2**63, 2**64 - 1):
            with self.subTest(offset=offset):
                self.assertEqual(self.reason(cursor_of([A, self.revision, offset])), "cursor_invalid")
        self.assertEqual(self.listed("--cursor", cursor_of([A, self.revision, 5]))["items"], [])

    def test_a_cursor_stale_after_the_manifest_is_rewritten(self):
        old = cursor_of([A, self.revision, 2])
        self.register(*(item(f"a{n:02d}", f"f{n:02d}.txt") for n in range(5)))
        self.assertEqual(self.reason(old), "cursor_stale")

    def test_limit_and_id_errors_carry_no_data(self):
        for limit in ("0", "51"):
            error = self.bad("task-artifacts", A, "--limit", limit, code="artifact_error")
            self.assertNotIn("data", error)
        self.assertNotIn("data", self.bad("task-artifacts", "bad/id", code="artifact_error"))
        self.assertNotIn("data", self.bad("task-artifacts", "ffffffff-ffff-4fff-8fff-ffffffffffff", code="artifact_error"))

    def test_a_task_without_a_manifest_has_the_revision_of_an_empty_file(self):
        self.add(B, started_at="2026-01-11T10:00:00+00:00")
        page = self.listed(task_id=B)
        self.assertEqual((page["items"], page["count"], page["state"], page["next_cursor"]), ([], 0, "available", None))
        self.assertEqual(self.listed("--cursor", cursor_of([B, EMPTY_REVISION, 0]), task_id=B)["items"], [])
        for value, reason in (
            ([B, EMPTY_REVISION, 1], "cursor_invalid"), ([B, EMPTY_REVISION, 2**64 - 1], "cursor_invalid"),
            ([B, EMPTY_REVISION, -1], "cursor_invalid"), ([B, self.revision, 0], "cursor_stale"),
            ([B, OTHER_REVISION, 0], "cursor_stale"), ([A, EMPTY_REVISION, 0], "cursor_stale"),
        ):
            with self.subTest(value=value):
                self.assertEqual(self.reason(cursor_of(value), task_id=B), reason)

    def test_the_cursor_bytes_are_the_default_json_dumps_spelling(self):
        cursor = self.listed("--limit", "2")["next_cursor"]
        self.assertEqual(base64.urlsafe_b64decode(cursor).decode(), json.dumps([A, self.revision, 2]))
        self.assertEqual(cursor, cursor_of([A, self.revision, 2]))


class PlanProvenanceTests(ArtifactCase):
    def seed_raw(self, op, arguments):
        return subprocess.run(
            [str(seeds.binary()), op, "--root", self.state], input=json.dumps(arguments).encode("utf-8"),
            capture_output=True, env=dict(seeds.ENVIRONMENT), timeout=seeds.TIMEOUT,
        )

    def served(self, *items):
        self.register(*items)
        return {entry["artifact_id"]: entry["provenance"] for entry in self.listed()["items"]}

    def test_a_plan_reference_serves_its_tool_use_id(self):
        provenance = self.served(item("p1", "p.md", PLAN))["p1"]
        self.assertEqual(provenance, {"method": PLAN, "task_id": A, "tool_use_id": "tool-use-1"})

    def test_a_plan_directory_and_identity_are_served_with_exact_integers(self):
        directory = self.repo_path + "/plans"
        provenance = self.served(item("p1", "p.md", PLAN, plan_directory=directory, plan_directory_identity=IDENTITY))["p1"]
        self.assertEqual(provenance, {"method": PLAN, "task_id": A, "tool_use_id": "tool-use-1",
                                      "plan_directory": directory, "plan_directory_identity": IDENTITY})

    def test_the_keys_are_in_the_order_of_the_reference_output(self):
        self.register(item("p1", "p.md", PLAN, plan_directory="/plans", plan_directory_identity=IDENTITY),
                      item("o1", "o.md"))
        done = self.read("task-artifacts", A, "--json")
        self.assertIn(
            '"provenance": {"method": "observed_backend_plan", "plan_directory": "/plans", "plan_directory_identity": '
            '{"device": 18446744073709551615, "inode": 9223372036854775808}, "task_id": "%s", "tool_use_id": "tool-use-1"}' % A,
            done.stdout)
        self.assertIn('"provenance": {"method": "verified_process_lineage", "task_id": "%s"}' % A, done.stdout)

    def test_other_methods_serve_no_plan_keys_and_a_page_mixes_them(self):
        served = self.served(item("o1", "o.md"), item("p1", "p.md", PLAN), item("e1", "e.md", "engine_output"))
        self.assertEqual(served["o1"], {"method": "verified_process_lineage", "task_id": A})
        self.assertEqual(served["e1"], {"method": "engine_output", "task_id": A})
        self.assertIn("tool_use_id", served["p1"])

    def test_the_longest_plan_evidence_is_served(self):
        directory = "/" + "d" * 4095
        tool = "t" * 256
        provenance = self.served(item("p1", "p.md", PLAN, tool_use_id=tool, plan_directory=directory,
                                      plan_directory_identity=IDENTITY, note="n" * 1000))["p1"]
        self.assertEqual(provenance["tool_use_id"], tool)
        self.assertEqual(provenance["plan_directory"], directory)

    def test_the_seed_port_refuses_malformed_plan_evidence(self):
        identity = {"device": 1, "inode": 2}
        bad = (
            ("missing tool_use_id", item("x", "x.md", PLAN, tool_use_id=None), "tool_use_id"),
            ("tool_use_id on another method", item("x", "x.md", tool_use_id="t"), "needs observed_backend_plan"),
            ("directory on another method", item("x", "x.md", plan_directory="/p", plan_directory_identity=identity), "needs observed_backend_plan"),
            ("long tool_use_id", item("x", "x.md", PLAN, tool_use_id="t" * 257), "256"),
            ("empty tool_use_id", item("x", "x.md", PLAN, tool_use_id=""), "256"),
            ("directory without identity", item("x", "x.md", PLAN, plan_directory="/p"), "come together"),
            ("identity without directory", item("x", "x.md", PLAN, plan_directory_identity=identity), "come together"),
            ("relative directory", item("x", "x.md", PLAN, plan_directory="p", plan_directory_identity=identity), "absolute"),
            ("parent part", item("x", "x.md", PLAN, plan_directory="/a/../b", plan_directory_identity=identity), "absolute"),
            ("unknown method", item("x", "x.md", method="other"), "provenance method"),
            ("unknown kind", item("x", "x.md", kind="k"), "artifact kind"),
            ("long directory", item("x", "x.md", PLAN, plan_directory="/" + "d" * 4096, plan_directory_identity=identity), "absolute"),
            ("negative identity", item("x", "x.md", PLAN, plan_directory="/p", plan_directory_identity={"device": -1, "inode": 2}), "device"),
            ("extra identity key", item("x", "x.md", PLAN, plan_directory="/p", plan_directory_identity=dict(identity, size=1)), "size"),
        )
        for label, entry, text in bad:
            if label == "missing tool_use_id":
                del entry["tool_use_id"]
            with self.subTest(label):
                done = self.seed_raw("write_artifacts", {"task_id": A, "items": [entry]})
                self.assertEqual(done.returncode, 1, done.stdout)
                self.assertEqual(done.stdout, b"")
                self.assertIn(text, done.stderr.decode("utf-8"))
        self.assertEqual(self.listed()["items"], [])

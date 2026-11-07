"""Black-box tests of how artifact paths are spelled and looked up: the kernel's spelling of a
firmlink or a case alias, lookup errors, text that is not UTF-8, `~` in a configured path and the
workflow-builder refusal."""

import json
import os
import sqlite3
import sys
import unittest

import seed_support as seeds
from history_support import ArtifactCase, item
from seed_support import A, B

DATA_VOLUME = "/System/Volumes/Data"


def case_sensitive(path):
    return not os.path.exists(os.path.join(os.path.dirname(path), os.path.basename(path).upper()))


class SpellingTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"), item("x2", "b.txt"))

    def check(self, parts, expected, env=None, base=None):
        path = os.path.join(base or self.repo_path, *parts)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as stream:
            stream.write(b"x")
        self.move_to(path, "x2")
        self.assertEqual(self.states(env=env)["x2"], expected, (parts, env))

    @unittest.skipUnless(sys.platform == "darwin", "firmlinks are a macOS volume feature")
    def test_a_firmlink_spelling_of_a_path_on_the_data_volume_is_changed(self):
        aliased = DATA_VOLUME + self.repo_path
        if not os.path.exists(aliased):
            self.skipTest("the scratch area is not on the data volume")
        path = self.file_at("plain.txt")
        self.move_to(path, "x2")
        self.assertEqual(self.states()["x2"], "available")
        self.move_to(DATA_VOLUME + path, "x2")
        self.assertEqual(self.states()["x2"], "changed")

    @unittest.skipUnless(sys.platform == "darwin", "firmlinks are a macOS volume feature")
    def test_a_repository_recorded_through_a_firmlink_still_judges_the_target(self):
        aliased = DATA_VOLUME + self.repo_path
        if not os.path.exists(aliased):
            self.skipTest("the scratch area is not on the data volume")
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (aliased, A))
        self.move_to(self.file_at("plain.txt"), "x2")
        self.assertEqual(self.states()["x2"], "available")

    def test_a_namespace_reached_through_a_case_alias_is_still_protected(self):
        os.makedirs(os.path.join(self.repo_path, "claudecfg"))
        if case_sensitive(os.path.join(self.repo_path, "claudecfg")):
            self.skipTest("the volume is case-sensitive")
        env = {"CLAUDE_CONFIG_DIR": os.path.join(self.repo_path, "CLAUDECFG")}
        self.check(("claudecfg", "a.txt"), "unavailable", env)
        self.check(("elsewhere", "a.txt"), "available", env)

    def test_a_forbidden_name_reached_through_a_case_alias_in_a_namespace_variable(self):
        os.makedirs(os.path.join(self.repo_path, "xdgdata", "opencode"))
        if case_sensitive(os.path.join(self.repo_path, "xdgdata")):
            self.skipTest("the volume is case-sensitive")
        env = {"XDG_DATA_HOME": os.path.join(self.repo_path, "XDGDATA")}
        self.check(("xdgdata", "opencode", "a.txt"), "unavailable", env)

    def test_an_unsearchable_directory_on_the_path_of_a_namespace_fails_closed(self):
        if os.geteuid() == 0:
            self.skipTest("root searches every directory")
        locked = os.path.join(self.repo_path, "locked")
        os.makedirs(os.path.join(locked, "cfg"))
        env = {"CLAUDE_CONFIG_DIR": os.path.join(locked, "cfg")}
        self.check(("plain", "a.txt"), "available", env)
        os.chmod(locked, 0)
        try:
            self.assertEqual(self.states(env=env)["x2"], "unavailable")
        finally:
            os.chmod(locked, 0o700)

    def test_a_namespace_below_a_regular_file_fails_closed_but_a_missing_one_does_not(self):
        plain = os.path.join(self.repo_path, "afile")
        with open(plain, "wb") as stream:
            stream.write(b"x")
        self.check(("plain", "a.txt"), "available", {"CLAUDE_CONFIG_DIR": os.path.join(self.repo_path, "nowhere", "cfg")})
        self.assertEqual(self.states(env={"CLAUDE_CONFIG_DIR": os.path.join(plain, "sub")})["x2"], "unavailable")

    def test_a_target_below_a_regular_file_is_unavailable_not_deleted(self):
        path = self.file_at("out", "a.txt")
        self.sql("UPDATE artifact_items SET path=? WHERE artifact_id='x2'", (os.path.join(path, "inner"),))
        self.assertEqual(self.states()["x2"], "unavailable")

    def test_a_missing_file_stays_deleted_beside_the_other_lookup_errors(self):
        self.sql("UPDATE artifact_items SET path=? WHERE artifact_id='x2'", (os.path.join(self.repo_path, "out", "gone.txt"),))
        self.assertEqual(self.states()["x2"], "deleted")

    def test_a_row_whose_path_text_is_not_utf_8_is_never_available_and_never_a_panic(self):
        """The row cannot be decoded, so the read fails or the item is not served as available
        before any path is looked up. The containment around the kernel's spelling (a non-UTF-8
        answer from F_GETPATH) cannot be reached from here: APFS and HFS+ will not store a
        non-UTF-8 name, and no fake is added."""
        bad = os.fsencode(self.repo_path) + b"/out/\xff\xfe.txt"
        for column, table, where in (("path", "artifact_items", "artifact_id='x2'"), ("repo_path", "task_facts", "1=1")):
            self.sql_bytes(f"UPDATE {table} SET {column}=CAST(? AS TEXT) WHERE {where}", bad if column == "path" else bad[:-9])
            done = self.read("task-artifacts", A, "--json")
            self.assertNotIn("panicked", done.stderr)
            self.assertIn(done.returncode, (0, 1), done.stderr)
            if done.returncode == 0:
                states = {e["artifact_id"]: e["availability"]["state"] for e in json.loads(done.stdout)["result"]["items"]}
                self.assertNotEqual(states.get("x2"), "available")
            else:
                self.assertEqual(json.loads(done.stdout)["error"]["code"], "state_incompatible")

    def sql_bytes(self, statement, value):
        db = sqlite3.connect(os.path.join(self.state, "store.sqlite3"))
        try:
            db.execute(statement, (value,))
            db.commit()
        finally:
            db.close()

    def test_a_tilde_in_a_variable_expands_against_the_real_home_not_the_opencode_test_home(self):
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.home, A))
        env = {"OPENCODE_TEST_HOME": os.path.join(self.home, "oh"), "XDG_DATA_HOME": "~/xdgdata"}
        self.check(("xdgdata", "opencode", "a.txt"), "unavailable", env, base=self.home)
        self.check(("oh", "xdgdata", "opencode", "a.txt"), "available", env, base=self.home)
        self.check(("oh", ".local", "share", "opencode", "a.txt"), "unavailable", env, base=self.home)


class WorkflowBuilderTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"))

    def blob(self, column, task_id=A):
        db = sqlite3.connect(os.path.join(self.state, "store.sqlite3"))
        try:
            return db.execute(f"SELECT {column} FROM task_headers WHERE id=?", (task_id,)).fetchone()[0]
        finally:
            db.close()

    def mark(self, column, value, task_id=A):
        document = json.loads(self.blob(column, task_id))
        document["workflow_builder"] = value
        self.sql(f"UPDATE task_headers SET {column}=? WHERE id=?", (json.dumps(document).encode(), task_id))

    def refused(self, task_id=A):
        error = self.bad("task-artifacts", task_id, code="artifact_error")
        self.assertEqual(error["message"], "Managed workflow artifact access is not supported")

    def summary(self, task_id=A):
        return self.good("status", task_id)["task"]["artifact_summary"]

    def test_a_builder_flag_in_the_header_or_the_status_blob_refuses(self):
        for column in ("header", "status"):
            with self.subTest(blob=column):
                self.setUp()
                self.mark(column, True)
                self.refused()
                self.assertEqual(self.summary(), {"count": 0, "state": "unavailable"})

    def test_a_builder_among_the_ancestors_refuses_the_descendant(self):
        self.add(B, continuation_of=A, started_at="2026-01-11T10:00:00+00:00")
        self.good("task-artifacts", B)
        self.mark("header", True)
        self.refused(B)
        self.assertEqual(self.summary(B), {"count": 0, "state": "unavailable"})

    def test_a_false_flag_does_not_refuse_and_keeps_the_detail_reference(self):
        for value in (False, 0, "", [], {}, None):
            with self.subTest(value=value):
                self.mark("header", value)
                self.assertEqual(self.good("task-artifacts", A)["result"]["state"], "available")
                self.assertIn("detail_ref", self.summary())

    def test_python_truthiness_decides_in_either_blob(self):
        """`if current.workflow_builder:` in the reference, on the uncoerced JSON value."""
        values = (True, 1, 2.5, -1, "true", "x", "0", " ", [1], [0], {"a": 1}, False, 0, 0.0, "", [], {}, None)
        for column in ("header", "status"):
            for value in values:
                with self.subTest(blob=column, value=value):
                    self.setUp()
                    self.mark(column, value)
                    if value:
                        self.refused()
                        self.assertEqual(self.summary(), {"count": 0, "state": "unavailable"})
                    else:
                        self.assertEqual(self.good("task-artifacts", A)["result"]["state"], "available")

    def test_a_blob_that_is_not_json_says_nothing(self):
        for column in ("status", "header"):
            with self.subTest(blob=column):
                self.setUp()
                self.sql(f"UPDATE task_headers SET {column}=? WHERE id=?", (b"not json at all", A))
                self.assertEqual(self.good("task-artifacts", A)["result"]["state"], "available")

"""Black-box tests of `harflow-ctl state-inspect` over the P5 classification table."""

import json
import os
import subprocess
import sys
import unittest

import ctl_rows
from ctl_table import ROWS
from ctl_support import CtlCase, fixture, one_document, python_marker, snapshot

RESULT_KEYS = {
    "cooperative_ownership_only", "engine_version", "message", "reason",
    "recovery_supported", "state_root", "status",
}
MISSING_MESSAGE = "State root has not been initialized"


class StateInspectTableTests(CtlCase):
    def test_every_row_of_the_classification_table(self):
        for name, build, status, reason in ROWS:
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    result = self.inspect(root)
                    self.assertEqual((result["status"], result["reason"]), (status, reason))
                    self.assertEqual(result["state_root"], root)
                    self.assertIs(result["recovery_supported"], False)
                    self.assertNotIn("recovery", result)
                    self.assertIs(result["cooperative_ownership_only"], True)
                    self.assertTrue(set(result) - RESULT_KEYS <= {"observed_store"}, result)
                    self.assertTrue(RESULT_KEYS <= set(result), result)
                    self.assertTrue(result["message"])
                finally:
                    self.cleanup()

    def test_table_rows_are_not_vacuous(self):
        statuses = {row[2] for row in ROWS}
        self.assertEqual(statuses, {"missing", "ready", "incompatible", "interrupted"})
        reasons = {row[3] for row in ROWS}
        self.assertEqual(reasons, {
            "missing_root", "compatible", "unmarked_nonempty", "foreign_store", "invalid_marker",
            "unsupported_layout", "newer_store", "recovery_interrupted", "unsafe_root",
            "infrastructure_unavailable",
        })


class StateInspectDocumentTests(CtlCase):
    def test_missing_root_strings_and_envelope(self):
        result = self.ctl("state-inspect", "--json")
        self.assertEqual(result.returncode, 0)
        document = one_document(result)
        self.assertEqual(
            {key: document[key] for key in ("product", "format", "v")},
            {"product": "harflow", "format": "harflow.ctl", "v": 1},
        )
        self.assertEqual(set(document), {"product", "format", "v", "product_version", "result"})
        inspected = document["result"]
        self.assertEqual(inspected["message"], MISSING_MESSAGE)
        self.assertEqual(inspected["reason"], "missing_root")
        self.assertEqual(inspected["status"], "missing")
        self.assertEqual(inspected["engine_version"], document["product_version"])
        self.assertEqual(set(inspected), RESULT_KEYS)
        self.assertFalse(os.path.lexists(self.state))

    def test_ready_result_matches_the_pinned_specimen(self):
        specimen = fixture("ctl-state-inspect-ready.json")["result"]
        os.mkdir(self.state)
        result = self.inspect()
        self.assertEqual(set(result), set(specimen))
        for key, value in specimen.items():
            self.assertIs(type(result[key]), type(value), key)
        for key in ("message", "reason", "status", "recovery_supported", "cooperative_ownership_only"):
            self.assertEqual(result[key], specimen[key], key)

    def test_incompatible_result_has_the_specimen_shape_except_recovery(self):
        specimen = fixture("ctl-state-inspect.json")["result"]
        self.write("stray", b"x")
        result = self.inspect()
        self.assertEqual(result["message"], specimen["message"])
        self.assertEqual(result["reason"], specimen["reason"])
        self.assertEqual(set(result), set(specimen) - {"recovery"})

    def test_plain_output_is_the_raw_indented_result_without_an_envelope(self):
        os.mkdir(self.state)
        structured = self.ctl("state-inspect", "--json")
        plain = self.ctl("state-inspect")
        self.assertEqual(plain.returncode, 0)
        self.assertEqual(plain.stderr, "")
        self.assertTrue(plain.stdout.endswith("}\n"))
        self.assertIn('\n  "', plain.stdout)
        document = json.loads(plain.stdout)
        self.assertEqual(document, json.loads(structured.stdout)["result"])
        for key in ("product", "format", "v", "product_version"):
            self.assertNotIn(key, document)

    def test_every_status_exits_zero(self):
        for name, build, status, _ in ROWS:
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    result = self.ctl("state-inspect", "--json", root=root)
                    self.assertEqual(result.returncode, 0, result.stderr)
                finally:
                    self.cleanup()

    def test_observed_store_of_a_python_marker(self):
        self.write(".harflow-store.json", python_marker(layout_version=2, product_version="0.0.9"))
        result = self.inspect()
        self.assertEqual(
            result["observed_store"],
            {"product": "harflow", "format": "harflow.store", "v": 1,
             "product_version": "0.0.9", "layout_version": 2},
        )

    def test_observed_store_echoes_only_short_text_and_integers(self):
        marker = json.loads(python_marker())
        marker.update(product_version="9" * 300, layout_version=1.5, extra="ignored")
        self.write(".harflow-store.json", json.dumps(marker))
        observed = self.inspect()["observed_store"]
        self.assertEqual(set(observed), {"product", "format", "v"})
        self.assertNotIn("extra", observed)

    def test_marker_size_boundary(self):
        by_name = {row[0]: row for row in ROWS}
        for name, reason in (("largest accepted marker", "unsupported_layout"),
                             ("oversize marker", "invalid_marker")):
            with self.subTest(marker=name):
                self.setUp()
                try:
                    by_name[name][1](self)
                    size = os.path.getsize(os.path.join(self.state, ".harflow-store.json"))
                    self.assertEqual(size, 16384 if reason == "unsupported_layout" else 16385)
                    self.assertEqual(self.inspect()["reason"], reason)
                finally:
                    self.cleanup()

    def test_journal_completed_or_committed_is_not_interrupted(self):
        planned = self.inspect_after(ctl_rows.interrupted_planned)
        self.assertEqual(planned["status"], "interrupted")
        for builder in (ctl_rows.committed_journal_with_matching_marker, ctl_rows.committed_large_journal):
            with self.subTest(builder=builder.__name__):
                result = self.inspect_after(builder, new=True)
                self.assertNotEqual(result["status"], "interrupted")
                self.assertNotEqual(result["reason"], "recovery_interrupted")
        # A completed journal that no marker commits is refused but is not an unfinished recovery.
        for builder in (ctl_rows.completed_journal, ctl_rows.completed_journal_on_rust_root):
            with self.subTest(builder=builder.__name__):
                result = self.inspect_after(builder, new=True)
                self.assertEqual((result["status"], result["reason"]), ("incompatible", "recovery_interrupted"))

    def test_interrupted_journal_authorizes_nothing(self):
        result = self.inspect_after(ctl_rows.interrupted_planned)
        self.assertEqual(result["status"], "interrupted")
        self.assertIs(result["recovery_supported"], False)
        self.assertNotIn("recovery", result)
        self.assertNotIn("confirmation_token", json.dumps(result))

    def generations(self):
        import sqlite3

        with open(os.path.join(self.state, "store.identity"), encoding="utf-8") as handle:
            identity = handle.read().split("\n")[2]
        connection = sqlite3.connect(os.path.join(self.state, "store.sqlite3"))
        try:
            stored = connection.execute("SELECT generation FROM store_identity").fetchone()[0]
        finally:
            connection.close()
        return identity, stored

    def test_marker_from_one_store_with_anothers_database_reports_ready(self):
        first = self.inspect_after(ctl_rows.rust_current)
        identity, stored = self.generations()
        self.assertEqual(identity, stored)
        self.cleanup()
        self.setUp()
        second = self.inspect_after(ctl_rows.rust_with_other_generation_database)
        identity, stored = self.generations()
        self.assertNotEqual(identity, stored)
        self.assertEqual((first["status"], first["reason"]), ("ready", "compatible"))
        self.assertEqual((second["status"], second["reason"]), ("ready", "compatible"))

    def test_root_replaced_by_a_symlink_between_runs_is_unsafe(self):
        ctl_rows.rust_current(self)
        self.assertEqual(self.inspect()["status"], "ready")
        moved = os.path.join(self.base, "moved")
        os.rename(self.state, moved)
        self.symlink(moved, self.state)
        before = snapshot(self.base)
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsafe_root"))
        self.assertEqual(snapshot(self.base), before)

    def test_symlinked_leaves_are_not_followed(self):
        for builder in (ctl_rows.identity_is_a_symlink, ctl_rows.database_is_a_symlink,
                        ctl_rows.marker_is_a_symlink, ctl_rows.journal_is_a_symlink):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    # The target is valid, so following it would change the answer.
                    result = self.inspect()
                    self.assertNotEqual((result["status"], result["reason"]), ("ready", "compatible"))
                    self.assertNotIn("observed_store", result)
                finally:
                    self.cleanup()

    def test_default_root_from_an_empty_home_matches_python(self):
        if os.name == "nt":
            self.skipTest("Windows resolves the home directory from other sources")
        probe = subprocess.run(
            [sys.executable, "-c", "from pathlib import Path; print(Path.home() / '.harflow')"],
            env={**os.environ, "HOME": ""}, capture_output=True, encoding="utf-8", check=True,
        )
        expected = os.path.normpath(probe.stdout.strip())
        self.assertEqual(expected, "/.harflow")
        result = self.ctl("version-info", "--json", root=None, home=None, env={"HOME": ""})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["state_root"], expected)

    def test_root_equal_to_home_is_unsafe_even_when_populated(self):
        self.write("note.txt", b"x", root=self.home)
        result = self.inspect(self.home)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsafe_root"))
        self.assertEqual(self.inspect(self.home + os.sep + ".")["reason"], "unsafe_root")
        os.mkdir(self.state)
        self.assertEqual(self.inspect(self.state)["status"], "ready")

    def test_read_only_root_is_never_interrupted(self):
        for builder in (ctl_rows.root_without_search_permission,
                        ctl_rows.root_without_search_permission_and_content):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    result = self.inspect()
                    self.assertEqual(result["status"], "incompatible")
                    self.assertEqual(result["reason"], "infrastructure_unavailable")
                finally:
                    self.cleanup()

    def test_missing_versus_existing_root_under_a_symlinked_ancestor(self):
        missing = self.inspect_after(ctl_rows.missing_under_symlink)
        self.assertEqual((missing["status"], missing["reason"]), ("missing", "missing_root"))
        self.assertTrue(missing["state_root"].endswith(os.path.join("link", "state")))
        self.assertFalse(os.path.lexists(os.path.join(self.base, "real", "state")))
        existing = self.inspect_after(ctl_rows.root_under_symlinked_ancestor, new=True)
        self.assertEqual((existing["status"], existing["reason"]), ("incompatible", "unsafe_root"))

    def test_state_root_is_reported_lexically_normalized(self):
        os.mkdir(self.state)
        selected = os.path.join(self.base, "x", "..", "state") + os.sep
        result = self.inspect(selected)
        self.assertEqual(result["state_root"], self.state)
        self.assertEqual(result["status"], "ready")

    def inspect_after(self, builder, new=False):
        if new:
            self.cleanup()
            self.setUp()
        root = builder(self) or self.state
        return self.inspect(root)


if __name__ == "__main__":
    unittest.main()

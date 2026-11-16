"""Recovery proposals of `harflow-ctl state-inspect` (plan 3.3): eligibility of the root: classification rows, safety and Python state."""

import unittest

from proposal_case import *  # noqa: F401,F403
from proposal_case import ProposalCase


class MissingAndReadyTests(ProposalCase):
    def test_missing_root_has_no_recovery_key(self):
        # S07 of the conformance runner: a missing root reports no recovery key at all.
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("missing", "missing_root"))
        self.assertEqual(set(result), RESULT_KEYS)
        assert_no_proposal(self, result)
        plain = self.ctl("state-inspect")
        self.assertEqual(plain.returncode, 0)
        self.assertNotIn("recovery", json.loads(plain.stdout))
        self.assertFalse(os.path.lexists(self.state))

    def test_ready_roots_have_no_recovery_key(self):
        builders = (
            ctl_rows.empty, ctl_rows.lock_only, ctl_rows.pending_marker, ctl_rows.legacy_dirs,
            ctl_rows.legacy_dirs_with_lock, ctl_rows.rust_current,
            ctl_rows.rust_with_other_generation_database,
        )
        for builder in builders:
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    result = self.inspect()
                    self.assertEqual(result["status"], "ready")
                    self.assertEqual(set(result), RESULT_KEYS)
                    assert_no_proposal(self, result)
                finally:
                    self.cleanup()


class EligibilityTableTests(ProposalCase):
    def test_every_row_of_the_classification_table_has_the_planned_proposal(self):
        for name, build, status, reason in ROWS:
            if name in SUSPECT_ROWS:
                continue
            with self.subTest(row=name):
                self.setUp()
                try:
                    root = build(self) or self.state
                    result = self.inspect(root)
                    self.assertEqual((result["status"], result["reason"]), (status, reason))
                    if name in PROPOSED_ROWS:
                        assert_proposal(self, result, root=root)
                    else:
                        assert_no_proposal(self, result)
                finally:
                    self.cleanup()

    def test_the_planned_rows_exist_in_the_table(self):
        names = {row[0] for row in ROWS}
        self.assertLessEqual(PROPOSED_ROWS | SUSPECT_ROWS, names)

    def test_store_state_facts_report_infrastructure_unavailable_with_a_proposal(self):
        # Plan 3.3: -wal missing, acceptance pending/failed, a fence, a watermark absent or
        # invalid. The verdict is the E1 verdict; the proposal is the new part.
        def missing_wal(case):
            os.remove(os.path.join(case.state, "store.sqlite3-wal"))

        def acceptance(state):
            def build(case):
                case.write("store.wal-acceptance", f"harflow-wal-acceptance-1\n{GENERATION}\n{state}\n")
            return build

        def fence(state):
            def build(case):
                case.write("store.fence", f"harflow-fence-1\n{GENERATION}\n{state}\n")
            return build

        def watermark(content):
            def build(case):
                os.remove(os.path.join(case.state, "store.watermark"))
                if content is not None:
                    case.write("store.watermark", content)
            return build

        rows = (
            ("missing wal", missing_wal),
            ("acceptance pending", acceptance("pending")),
            ("acceptance failed", acceptance("failed")),
            ("fence failed", fence("failed")),
            ("fence commit lost", fence("commit_lost")),
            ("watermark absent", watermark(None)),
            ("watermark all zero", watermark(bytes(8192))),
        )

        def check(build):
            rust_root(self.state)
            build(self)
            result = self.inspect()
            self.assertEqual((result["status"], result["reason"]), ("incompatible", "infrastructure_unavailable"))
            assert_proposal(self, result)

        self.each(rows, check)


class FactRootSafeTests(ProposalCase):
    def test_unsafe_roots_have_no_proposal_and_the_same_layout_elsewhere_has_one(self):
        self.stray(self.home)
        result = self.inspect(self.home)
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "unsafe_root"))
        assert_no_proposal(self, result)
        self.stray()
        assert_proposal(self, self.inspect())

    def test_a_symlinked_root_and_a_symlinked_ancestor_have_no_proposal(self):
        for builder in (ctl_rows.root_is_symlink, ctl_rows.root_under_symlinked_ancestor,
                        ctl_rows.root_is_a_file, ctl_rows.root_is_dangling_symlink):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    root = builder(self) or self.state
                    result = self.inspect(root)
                    self.assertEqual(result["reason"], "unsafe_root")
                    assert_no_proposal(self, result)
                finally:
                    self.cleanup()

    def test_a_root_that_cannot_be_searched_has_no_proposal(self):
        for builder in (ctl_rows.root_without_search_permission,
                        ctl_rows.root_without_search_permission_and_content):
            with self.subTest(builder=builder.__name__):
                self.setUp()
                try:
                    builder(self)
                    result = self.inspect()
                    self.assertEqual(result["reason"], "infrastructure_unavailable")
                    assert_no_proposal(self, result)
                finally:
                    self.cleanup()


class FactPythonStateTests(ProposalCase):
    def test_python_state_is_not_proposed_and_the_verdict_is_unchanged(self):
        def python_marker_root(case):
            case.write(MARKER, python_marker())
            return "unsupported_layout"

        def python_layout_two(case):
            case.write(MARKER, python_marker(layout_version=2))
            return "unsupported_layout"

        def tasks_with_data(name):
            def build(case):
                case.stray()
                case.write(f"{name}/record", b"{}")
                return "unmarked_nonempty"
            return build

        rows = (
            ("python marker", python_marker_root),
            ("python marker, layout 2", python_layout_two),
            ("tasks with a record", tasks_with_data("tasks")),
            ("workflow-runs with a record", tasks_with_data("workflow-runs")),
            ("workflow-leases with a record", tasks_with_data("workflow-leases")),
            ("tasks with a nested record", lambda case: (case.write("tasks/a/b/c", b"x"), "unmarked_nonempty")[1]),
        )

        def check(build):
            reason = build(self)
            result = self.inspect()
            self.assertEqual((result["status"], result["reason"]), ("incompatible", reason))
            assert_no_proposal(self, result)

        self.each(rows, check)

    def test_state_that_is_not_python_state_is_proposed(self):
        def empty_directories(case):
            for name in ("tasks", "workflow-runs", "workflow-leases"):
                os.makedirs(os.path.join(case.state, name))
            case.stray()

        def other_legacy_names(case):
            case.write("workflows/item", b"x")
            case.write("workflow-owners/item", b"x")

        def reference_name_as_file(case):
            case.write("tasks", b"x")

        def reference_name_nested(case):
            case.write("outer/tasks/record", b"x")

        rows = (
            ("empty reference directories", empty_directories),
            ("workflows and workflow-owners with content", other_legacy_names),
            ("a file named tasks", reference_name_as_file),
            ("tasks nested deeper", reference_name_nested),
        )

        def check(build):
            build(self)
            assert_proposal(self, self.inspect())

        self.each(rows, check)

    def test_a_journal_of_another_engine_is_python_state(self):
        for version in ("0.0.11", "0.0.14", "0.0.13+python"):
            with self.subTest(version=version):
                self.setUp()
                try:
                    self.unfinished_journal("planned", version=version)
                    result = self.inspect()
                    self.assertEqual((result["status"], result["reason"]), ("interrupted", "recovery_interrupted"))
                    assert_no_proposal(self, result)
                finally:
                    self.cleanup()


if __name__ == "__main__":
    unittest.main()

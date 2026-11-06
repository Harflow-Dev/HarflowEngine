"""Usage errors, state-root selection and the private-mode guard for harflow-ctl."""

import json
import os
import re
import unittest

from ctl_support import INVALID_ROOT, CtlCase, fixture, one_document, snapshot

PRIVATE_MODES = [
    "--fixture-fixed-protocol-helper",
    "--fixture-claude-qualification-parent",
    "--fixture-claude-qualification-owner",
    "--fixture-claude-qualification-writer",
    "--fixture-claude-qualification-bridge",
    "--fixture-exec-bridge-witness-v1",
    "--fixture-exec-image-witness-v1",
    "--internal-parent-fixture-v1",
    "--internal-owner-fixture-v1",
    "--internal-effect-fixture-v1",
    "--internal-effect-saturation-fixture-v1",
    "--internal-storage-writer-fixture-v1",
    "--internal-isolated-owner-fixture-v1",
]


class UsageTests(CtlCase):
    def test_missing_command_matches_the_pinned_usage_specimen(self):
        specimen = fixture("ctl-usage.json")
        result = self.ctl("--json")
        self.assertEqual(result.returncode, 2)
        document = one_document(result)
        self.assertEqual(set(document), set(specimen))
        self.assertEqual(document["error"], specimen["error"])
        for key in ("product", "format", "v"):
            self.assertEqual(document[key], specimen[key])
        self.assertEqual(
            result.stderr, "harflow-ctl: error: the following arguments are required: command\n"
        )

    def test_unknown_and_later_paths_are_invalid_choices(self):
        for command in ("list", "state-recover", "status", "task-events", "bogus", "setup"):
            with self.subTest(command=command):
                result = self.ctl(command, "--json")
                self.assertEqual(result.returncode, 2)
                message = one_document(result)["error"]["message"]
                self.assertIn("invalid choice", message)
                self.assertIn(f"'{command}'", message)
                self.assertIn("invalid choice", result.stderr)

    def test_extra_arguments_are_unrecognized(self):
        cases = [
            (["state-inspect", "extra"], "extra"),
            (["state-inspect", "--bogus"], "--bogus"),
            (["--bogus", "state-inspect"], "--bogus"),
            (["version-info", "extra"], "extra"),
            (["state-inspect", "--bogus=1"], "--bogus=1"),
        ]
        for args, token in cases:
            with self.subTest(args=args):
                result = self.ctl(*args, "--json")
                self.assertEqual(result.returncode, 2)
                message = one_document(result)["error"]["message"]
                self.assertIn("unrecognized arguments", message)
                self.assertIn(token, message)

    def test_option_equals_value_and_double_dash_are_parsed(self):
        baseline = self.ctl("state-inspect", "--json")
        self.assertEqual(baseline.returncode, 0)
        after_terminator = self.ctl("--", "state-inspect", "--json")
        self.assertEqual(after_terminator.returncode, 0, after_terminator.stderr)
        self.assertEqual(after_terminator.stdout, baseline.stdout)
        positional = self.ctl("state-inspect", "--", "--json")
        self.assertEqual(positional.returncode, 2)
        self.assertIn("unrecognized arguments: --json", positional.stderr)
        attached = self.ctl("state-inspect", "--json=1")
        self.assertEqual(attached.returncode, 2)
        self.assertIn("ignored explicit argument", attached.stderr)
        prefix = self.ctl("state-inspect", "--js")
        self.assertEqual(json.loads(prefix.stdout)["format"], "harflow.ctl")

    def test_json_goes_to_stdout_only_when_requested_and_prose_always_to_stderr(self):
        for position in (["--json", "bogus"], ["bogus", "--json"], ["state-inspect", "x", "--json"]):
            with self.subTest(position=position):
                result = self.ctl(*position)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(one_document(result)["error"]["code"], "usage")
                self.assertTrue(result.stderr.startswith("harflow-ctl: error: "))
        for args in (["bogus"], [], ["state-inspect", "x"]):
            with self.subTest(args=args):
                result = self.ctl(*args)
                self.assertEqual((result.returncode, result.stdout), (2, ""))
                self.assertTrue(result.stderr.startswith("harflow-ctl: error: "))

    def test_success_writes_nothing_to_stderr(self):
        for args in (["state-inspect"], ["state-inspect", "--json"], ["version-info"]):
            self.assertEqual(self.ctl(*args).stderr, "")

    def test_help_succeeds_on_stdout(self):
        for args in (["-h"], ["--help"], ["state-inspect", "-h"], ["version-info", "--help"]):
            with self.subTest(args=args):
                result = self.ctl(*args)
                self.assertEqual(result.returncode, 0)
                self.assertTrue(result.stdout.startswith("usage: harflow-ctl"))
                self.assertEqual(result.stderr, "")

    def test_usage_errors_have_no_effect(self):
        before = snapshot(self.base)
        for args in (["list"], ["state-recover"], ["state-inspect", "x"], []):
            self.assertEqual(self.ctl(*args).returncode, 2)
        self.assertEqual(snapshot(self.base), before)


class StateRootTests(CtlCase):
    def selected(self, root):
        result = self.ctl("version-info", "--json", root=root)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)["state_root"]

    def test_unset_uses_home_dot_harflow(self):
        result = self.ctl("state-inspect", "--json", root=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            one_document(result)["result"]["state_root"], os.path.join(self.home, ".harflow")
        )
        self.assertFalse(os.path.lexists(os.path.join(self.home, ".harflow")))

    def test_selected_root_is_normalized_lexically(self):
        base = self.base
        cases = {
            "doubled separators": base + os.sep + "a" + os.sep + os.sep + "state",
            "current directory": os.path.join(base, ".", "a", ".", "state"),
            "parent directory": os.path.join(base, "a", "b", "..", "state"),
            "trailing separator": os.path.join(base, "a", "state") + os.sep,
            "parent past the top": os.path.join(base, *[".."] * 40, "state"),
        }
        for name, root in cases.items():
            with self.subTest(name=name):
                expected = os.path.normpath(root)
                self.assertEqual(self.selected(root), expected)

    def test_normalization_does_not_resolve_symlinks(self):
        target = os.path.join(self.base, "target")
        os.mkdir(target)
        link = os.path.join(self.base, "link")
        self.symlink(target, link)
        root = os.path.join(link, "..", "state")
        self.assertEqual(self.selected(root), os.path.join(self.base, "state"))
        self.assertEqual(self.selected(os.path.join(link, "state")), os.path.join(link, "state"))

    def test_invalid_roots_are_usage_errors_before_parsing(self):
        cases = {"relative": os.path.join("relative", "state"), "dot": ".", "empty": ""}
        for name, root in cases.items():
            for command in (["state-inspect", "--json"], ["version-info", "--json"], ["list", "--json"], ["--json"]):
                with self.subTest(name=name, command=command):
                    result = self.ctl(*command, root=root)
                    self.assertEqual(result.returncode, 2)
                    document = one_document(result)
                    self.assertEqual(document["error"], {"code": "usage", "message": INVALID_ROOT})
                    self.assertIn(INVALID_ROOT, result.stderr)

    def test_invalid_root_wins_over_a_bad_command_without_json_output(self):
        result = self.ctl("bogus", root="relative")
        self.assertEqual((result.returncode, result.stdout), (2, ""))
        self.assertIn(INVALID_ROOT, result.stderr)
        self.assertNotIn("invalid choice", result.stderr)

    def test_invalid_root_format_follows_the_command(self):
        version = one_document(self.ctl("version-info", "--json", root="relative"))
        self.assertEqual(version["format"], "harflow.version-info")
        inspect = one_document(self.ctl("state-inspect", "--json", root="relative"))
        self.assertEqual(inspect["format"], "harflow.ctl")

    def test_help_ignores_an_invalid_root(self):
        for args in (["-h"], ["--help"], ["state-inspect", "-h"], ["version-info", "-h"]):
            with self.subTest(args=args):
                result = self.ctl(*args, root="relative")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(result.stdout.startswith("usage:"))

    def test_invalid_root_creates_nothing(self):
        before = snapshot(self.base)
        self.ctl("state-inspect", root="relative")
        self.assertEqual(snapshot(self.base), before)


class PrivateModeGuardTests(CtlCase):
    def test_baseline_the_binary_runs_a_public_command(self):
        self.assertEqual(self.ctl("version-info").returncode, 0)

    def test_private_fixture_modes_are_usage_errors_with_no_effect(self):
        for mode in PRIVATE_MODES:
            for extra in ([], ["extra"], ["a", "b"]):
                with self.subTest(mode=mode, extra=extra):
                    before = snapshot(self.base)
                    result = self.ctl(mode, *extra)
                    self.assertEqual(result.returncode, 2)
                    self.assertEqual(result.stdout, "")
                    self.assertRegex(result.stderr, r"^harflow-ctl: error: ")
                    self.assertNotRegex(result.stderr, re.compile("panic", re.I))
                    self.assertEqual(snapshot(self.base), before)

    def test_private_modes_are_not_accepted_as_commands_or_after_commands(self):
        for mode in PRIVATE_MODES[:3]:
            for args in (["state-inspect", mode], ["version-info", mode], [mode, "state-inspect"]):
                with self.subTest(args=args):
                    result = self.ctl(*args, "--json")
                    self.assertEqual(result.returncode, 2)
                    self.assertEqual(one_document(result)["error"]["code"], "usage")

    def test_private_mode_leaves_a_selected_root_absent(self):
        result = self.ctl("--internal-owner-fixture-v1")
        self.assertEqual(result.returncode, 2)
        self.assertFalse(os.path.lexists(self.state))


if __name__ == "__main__":
    unittest.main()

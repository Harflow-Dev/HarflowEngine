"""Black-box tests of `harflow-ctl version-info`, judged against the pinned specimen."""

import json
import os
import platform
import re
import sys
import unittest

from ctl_support import CtlCase, fixture, one_document, snapshot

VERSION = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(\+[0-9A-Za-z.-]+)?$")


class VersionInfoTests(CtlCase):
    def document(self, *args, **options):
        result = self.ctl("version-info", "--json", *args, **options)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        return one_document(result)

    def test_structure_matches_the_pinned_specimen(self):
        specimen = fixture("version-info.json")
        document = self.document()
        self.assertEqual(set(document), set(specimen))
        for key, value in specimen.items():
            self.assertIs(type(document[key]), type(value), key)
        for key in ("product", "format", "component", "v", "contracts"):
            self.assertEqual(document[key], specimen[key], key)
        self.assertIs(type(document["v"]), int)
        self.assertTrue(all(isinstance(item, str) and item for item in document["capabilities"]))

    def test_product_version_is_a_live_compatible_semantic_version(self):
        version = self.document()["product_version"]
        match = VERSION.match(version)
        self.assertIsNotNone(match, version)
        self.assertEqual(match.group(1, 2), ("0", "0"))
        self.assertLessEqual(len(version.encode("utf-8")), 256)
        self.assertEqual(version, "0.0.13+rust")

    def test_capabilities_are_exactly_the_advertised_set(self):
        capabilities = self.document()["capabilities"]
        expected = ["ctl.v1", "version-info.v1"]
        if sys.platform == "darwin" and platform.machine() == "arm64":
            expected = [
                "interaction-events.v1", "ctl.v1", "version-info.v1", "statistics-page.v1",
                "task-event-page.v1", "task-prompt.v1", "task-successors.v1", "task-artifacts.v1",
            ]
        self.assertEqual(capabilities, expected)
        self.assertEqual(len(capabilities), len(set(capabilities)))
        for withheld in ("setup.v1", "events.v1", "state-recovery.v1", "statistics-cost-model.v1"):
            self.assertNotIn(withheld, capabilities)

    def test_contracts_list_all_three_formats(self):
        contracts = self.document()["contracts"]
        self.assertEqual(
            contracts,
            {
                "ctl": {"format": "harflow.ctl", "v": 1},
                "setup": {"format": "harflow.setup", "v": 1},
                "events": {"format": "harflow.event", "v": 1},
            },
        )
        for contract in contracts.values():
            self.assertIs(type(contract["v"]), int)

    def test_os_and_architecture_follow_the_python_names(self):
        document = self.document()
        self.assertEqual(document["os"], sys.platform)
        self.assertEqual(document["architecture"], platform.machine())

    def test_state_root_is_the_selected_root_lexically(self):
        self.assertEqual(self.document()["state_root"], self.state)
        through_link = os.path.join(self.base, "link")
        self.symlink(self.base, through_link)
        selected = os.path.join(through_link, "state")
        result = self.ctl("version-info", "--json", root=selected)
        self.assertEqual(json.loads(result.stdout)["state_root"], selected)

    def test_default_state_root_is_home_dot_harflow(self):
        result = self.ctl("version-info", "--json", root=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            json.loads(result.stdout)["state_root"], os.path.join(self.home, ".harflow")
        )

    def test_plain_output_is_one_line_naming_the_version(self):
        result = self.ctl("version-info")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr, "")
        version = self.document()["product_version"]
        self.assertEqual(result.stdout, f"harflow-ctl {version}\n")
        self.assertNotIn("{", result.stdout)

    def test_output_fits_the_handshake_budget(self):
        result = self.ctl("version-info", "--json")
        self.assertLess(len(result.stdout.encode("utf-8")), 64 * 1024)
        long_root = os.path.join(self.base, "r" * 200, "s" * 200)
        result = self.ctl("version-info", "--json", root=long_root)
        self.assertEqual(result.returncode, 0)
        self.assertLess(len(result.stdout.encode("utf-8")), 64 * 1024)

    def test_has_no_filesystem_effect(self):
        for root in (self.state, os.path.join(self.home, "absent", ".harflow")):
            before = snapshot(self.base)
            for args in (("version-info", "--json"), ("version-info",)):
                self.assertEqual(self.ctl(*args, root=root).returncode, 0)
            self.assertEqual(snapshot(self.base), before)
            self.assertFalse(os.path.lexists(root))

    def test_does_not_read_an_existing_unusable_store(self):
        self.write(".harflow-store.json", b"\xff not json")
        self.assertEqual(self.document()["state_root"], self.state)

    def test_stale_task_markers_are_ignored(self):
        clean = self.ctl("version-info", "--json")
        stale = self.ctl(
            "version-info", "--json",
            env={"HARFLOW_TASK_ID": "stale-task-id", "HARFLOW_DEPTH": "3"},
        )
        self.assertEqual(stale.returncode, 0)
        self.assertEqual(stale.stdout, clean.stdout)
        self.assertEqual(stale.stderr, "")

    def test_json_flag_can_be_given_by_prefix_and_equals_forms_fail_cleanly(self):
        prefixed = self.ctl("version-info", "--js")
        self.assertEqual(prefixed.returncode, 0)
        self.assertEqual(json.loads(prefixed.stdout)["format"], "harflow.version-info")
        value = self.ctl("version-info", "--json=yes")
        self.assertEqual(value.returncode, 2)
        self.assertIn("ignored explicit argument", value.stderr)

    def test_usage_error_uses_the_version_info_format(self):
        result = self.ctl("version-info", "--json", "extra")
        self.assertEqual(result.returncode, 2)
        document = one_document(result)
        self.assertEqual(document["format"], "harflow.version-info")
        self.assertEqual(document["error"]["code"], "usage")
        self.assertIn("unrecognized arguments: extra", document["error"]["message"])
        self.assertIn("unrecognized arguments: extra", result.stderr)


if __name__ == "__main__":
    unittest.main()

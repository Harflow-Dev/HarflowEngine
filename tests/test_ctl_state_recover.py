"""Public B6 recovery against isolated Rust roots, never Python-owned state.

Set HARFLOW_B6_CTL_BIN to exercise the coordinator's exact release artifact;
otherwise use the suite's usual debug binary. No binary is built at import time.
"""
import json
import os
import platform
import shutil
import subprocess
import sys
import unittest
from pathlib import Path

import ctl_support
from ctl_support import CtlCase, fixture, journal, one_document, python_marker, snapshot
from proposal_support import local_decode, local_encode
from seed_support import binary as seed_binary

SUPPORTED = platform.system() == "Darwin" and platform.machine() == "arm64"


class RecoveryCase(CtlCase):
    def setUp(self):
        super().setUp()
        selected = os.environ.get("HARFLOW_B6_CTL_BIN")
        if selected:
            previous = ctl_support._binary
            ctl_support._binary = Path(selected).resolve(strict=True)
            self.addCleanup(setattr, ctl_support, "_binary", previous)

    def recover(self, proposal, *extra, root="default", operation=None, token=None):
        return self.ctl("state-recover", "--operation-id",
                        operation if operation is not None else proposal["operation_id"],
                        "--confirmation-token",
                        token if token is not None else proposal["confirmation_token"],
                        *extra, root=root)

    def refused(self, done, code, reason):
        self.assertEqual(done.returncode, 1, done)
        self.assertTrue(done.stderr.endswith("\n"), done.stderr)
        document = one_document(done)
        specimen = fixture("ctl-state-recover-error.json")
        self.assertEqual(set(document), set(specimen))
        self.assertEqual(set(document["error"]), set(specimen["error"]))
        self.assertEqual(document["error"]["code"], code)
        self.assertEqual(document["error"]["data"]["reason"], reason)
        return document["error"]["data"]

    def seeded(self):
        done = subprocess.run([str(seed_binary()), "initialize_empty", "--root", self.state],
                              input=b"{}", capture_output=True,
                              env={"PATH": "/usr/bin:/bin"}, timeout=60)
        self.assertEqual(done.returncode, 0, done.stderr)
        marker = Path(self.state, "store.identity").read_text()
        generation = marker.splitlines()[2]
        self.write("store.fence", f"harflow-fence-1\n{generation}\nfailed\n")
        self.write("nested/blob", b"opaque\x00\xff")
        return self.inspect()["recovery"]


@unittest.skipUnless(SUPPORTED, "public recovery is experimental macOS arm64 only")
class StateRecoverTests(RecoveryCase):
    def test_archives_bytes_and_returns_contract_shape_then_fresh_store(self):
        proposal = self.seeded()
        result = self.recover(proposal, "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        document = one_document(result)
        specimen = fixture("ctl-state-recover.json")
        self.assertEqual(set(document), set(specimen))
        value = document["result"]
        self.assertEqual(set(value), set(specimen["result"]))
        self.assertEqual(value["operation_id"], proposal["operation_id"])
        self.assertEqual(value["backup_id"], proposal["operation_id"])
        self.assertEqual((value["status"], value["phase"]), ("completed", "completed"))
        self.assertIs(value["recovery_supported"], True)
        self.assertEqual(Path(value["backup_path"], "data/nested/blob").read_bytes(), b"opaque\x00\xff")
        self.assertFalse(Path(self.state, "nested").exists())
        self.assertEqual(self.inspect()["reason"], "compatible")
        # A healthy bare Store has no seeded task-catalog coverage authority.
        # docs/task-reads.md specifies fail-closed unavailability for that dataset.
        listed = self.ctl("list", "--json")
        unavailable = self.refused(listed, "state_incompatible", "infrastructure_unavailable")
        self.assertIs(unavailable["recovery_supported"], False)
        self.write("new-after-r8", b"preserve")
        before = snapshot(self.state)
        repeat = self.recover(proposal)
        self.assertEqual(repeat.returncode, 0, repeat.stderr)
        self.assertEqual(json.loads(repeat.stdout), value)
        self.assertEqual(snapshot(self.state), before)

    def test_confirmation_mismatch_never_mutates(self):
        proposal = self.seeded()
        decoded = local_decode(proposal["confirmation_token"])
        tokens = ["!", "x" * 8193]
        for key, replacement in (("state_root", self.state + "-other"),
                                 ("engine_version", "0.0.0+other"),
                                 ("operation_id", "f" * 32)):
            tokens.append(local_encode(dict(decoded, **{key: replacement})))
        for token in tokens:
            with self.subTest(token=token[:30]):
                before = snapshot(self.base)
                self.refused(self.recover(proposal, "--json", token=token),
                             "state_recovery_stale", "stale_confirmation")
                self.assertEqual(snapshot(self.base), before)
        before = snapshot(self.base)
        self.refused(self.recover(proposal, "--json", operation="invalid"),
                     "state_recovery_stale", "stale_confirmation")
        self.assertEqual(snapshot(self.base), before)

    def test_deleted_selected_root_is_stale_without_recreation(self):
        proposal = self.seeded()
        shutil.rmtree(self.state)
        before = snapshot(self.base)
        data = self.refused(self.recover(proposal, "--json"),
                            "state_recovery_stale", "stale_confirmation")
        self.assertIs(data["recovery_supported"], True)
        self.assertFalse(Path(self.state).exists())
        self.assertFalse(Path(proposal["backup_path"]).exists())
        self.assertEqual(snapshot(self.base), before)

    def test_unreadable_selected_root_reports_infrastructure_failure(self):
        proposal = self.seeded()
        before = snapshot(self.base)
        original_mode = os.stat(self.base).st_mode & 0o7777
        try:
            # Deny ancestor traversal: chmod(root, 0) alone still permits lstat(root).
            os.chmod(self.base, 0)
            try:
                os.lstat(self.state)
            except PermissionError:
                pass
            else:
                self.skipTest("ancestor permissions do not deny root metadata to this user")
            done = self.ctl("state-recover", "--operation-id", proposal["operation_id"],
                            "--confirmation-token", proposal["confirmation_token"],
                            "--json", cwd="/", home="/")
            data = self.refused(done, "state_recovery_failed", "infrastructure_unavailable")
            self.assertIs(data["recovery_supported"], True)
        finally:
            os.chmod(self.base, original_mode)
        self.assertFalse(Path(proposal["backup_path"]).exists())
        self.assertEqual(snapshot(self.base), before)

    def test_unreadable_legacy_directory_is_infrastructure_failure(self):
        self.write("foreign.data", b"eligible Rust confirmation")
        proposal = self.inspect()["recovery"]
        legacy = Path(self.state, "tasks")
        legacy.mkdir()
        original_mode = legacy.stat().st_mode & 0o7777
        before = snapshot(self.base)
        try:
            os.chmod(legacy, 0)
            try:
                list(legacy.iterdir())
            except PermissionError:
                pass
            else:
                self.skipTest("directory permissions do not deny listing to this user")
            done = self.recover(proposal, "--json")
            data = self.refused(done, "state_recovery_failed", "infrastructure_unavailable")
            self.assertIs(data["recovery_supported"], True)
        finally:
            os.chmod(legacy, original_mode)
        self.assertEqual(snapshot(self.base), before)
        self.assertFalse(Path(proposal["backup_path"]).exists())
        for lock in ("store.lock", "io.lock", ".harflow-store.lock"):
            self.assertFalse(Path(self.state, lock).exists())

    def test_python_state_added_after_confirmation_refuses_before_mutation(self):
        cases = ("layout1", "other-layout", "tasks", "workflow-runs",
                 "workflow-leases", "python-journal")
        for kind in cases:
            with self.subTest(kind=kind):
                root = os.path.join(self.base, "python-gate-" + kind)
                self.write("foreign.data", b"Rust eligible payload", root=root)
                proposal = self.inspect(root)["recovery"]
                if kind in ("layout1", "other-layout"):
                    self.write(".harflow-store.json",
                               python_marker(layout_version=1 if kind == "layout1" else 3), root=root)
                elif kind == "python-journal":
                    # Positive Python provenance: valid public journal with its
                    # original Python engine version in the bound proposal.
                    decoded = local_decode(proposal["confirmation_token"])
                    decoded["engine_version"] = "0.0.13"
                    self.write(".harflow-recovery.json",
                               journal(root, operation=proposal["operation_id"],
                                       proposal=decoded, root_identity=decoded["root_identity"],
                                       engine_version="0.0.13"), root=root)
                else:
                    self.write(kind + "/evidence", b"Python owned state", root=root)
                before = snapshot(self.base)
                done = self.recover(proposal, "--json", root=root)
                data = self.refused(done, "state_recovery_unsafe", "unsafe_root")
                self.assertIs(data["recovery_supported"], False)
                message = one_document(done)["error"]["message"].lower()
                self.assertIn("python", message)
                self.assertIn("fresh", message)
                self.assertIn("root", message)
                self.assertEqual(snapshot(self.base), before)
                self.assertFalse(Path(proposal["backup_path"]).exists())
                for lock in ("store.lock", "io.lock", ".harflow-store.lock"):
                    self.assertFalse(Path(root, lock).exists())

    def test_plain_failure_has_no_stdout(self):
        proposal = self.seeded()
        before = snapshot(self.base)
        result = self.recover(proposal, token="!")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertTrue(result.stderr.startswith("harflow-ctl: error: "))
        self.assertEqual(snapshot(self.base), before)

    def test_changed_inventory_refuses_and_preserves_bytes(self):
        proposal = self.seeded()
        self.write("nested/blob", b"changed")
        before = snapshot(self.base)
        self.refused(self.recover(proposal, "--json"),
                     "state_recovery_stale", "stale_confirmation")
        self.assertEqual(snapshot(self.base), before)
        self.assertEqual(Path(self.state, "nested/blob").read_bytes(), b"changed")

    def test_layout2_refuses_before_invalid_operation_and_token(self):
        self.write(".harflow-store.json", python_marker(layout_version=2))
        before = snapshot(self.base)
        self.refused(self.recover({}, "--json", operation="invalid", token="!"),
                     "state_recovery_unsafe", "phase_layout_recovery_unsupported")
        self.assertEqual(snapshot(self.base), before)

    def test_competing_process_lock_refuses_without_archiving(self):
        proposal = self.seeded()
        code = ("import fcntl,sys; f=open(sys.argv[1],'r+'); "
                "fcntl.lockf(f,fcntl.LOCK_EX); print('ready',flush=True); sys.stdin.read(1)")
        holder = subprocess.Popen([sys.executable, "-c", code, str(Path(self.state, "store.lock"))],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(holder.stdout.readline().strip(), "ready")
            self.refused(self.recover(proposal, "--json"), "state_busy", "active_owner")
            self.assertFalse(Path(proposal["backup_path"]).exists())
            self.assertEqual(Path(self.state, "nested/blob").read_bytes(), b"opaque\x00\xff")
        finally:
            holder.communicate("x", timeout=5)

    def test_unique_option_prefixes_reach_operational_validation(self):
        done = self.ctl("state-recover", "--op", "a" * 32, "--confirmation", "!", "--j")
        self.refused(done, "state_recovery_stale", "stale_confirmation")
        self.assertFalse(Path(self.state).exists())

    def test_required_flags_and_extra_arguments(self):
        for arguments in ((), ("--operation-id", "a" * 32),
                          ("--confirmation-token", "x"),
                          ("--operation-id", "a" * 32, "--confirmation-token", "x", "extra")):
            with self.subTest(arguments=arguments):
                result = self.ctl("state-recover", *arguments)
                self.assertEqual(result.returncode, 2, result)
                self.assertEqual(result.stdout, "")
                self.assertFalse(Path(self.state).exists())


class RecoveryCapabilityTests(RecoveryCase):
    def test_capability_order_and_platform_refusal(self):
        result = one_document(self.ctl("version-info", "--json"))
        capabilities = result["capabilities"]
        if SUPPORTED:
            self.assertEqual(capabilities[capabilities.index("version-info.v1") + 1], "state-recovery.v1")
        else:
            self.assertNotIn("state-recovery.v1", capabilities)
            done = self.ctl("state-recover", "--operation-id", "a" * 32,
                            "--confirmation-token", "x", "--json")
            self.assertEqual(done.returncode, 2)
            self.assertIn("invalid choice", done.stderr)
            self.assertFalse(Path(self.state).exists())

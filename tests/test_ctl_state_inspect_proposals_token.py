"""Recovery proposals of `harflow-ctl state-inspect` (plan 3.3): proposal fields and confirmation tokens."""

import unittest

from proposal_case import *  # noqa: F401,F403
from proposal_case import ProposalCase


class ProposalFieldTests(ProposalCase):
    def test_a_finished_uncommitted_journal_of_our_engine_gives_a_fresh_proposal_with_its_digest(self):
        self.unfinished_journal("completed")
        path = os.path.join(self.state, JOURNAL)
        result = self.inspect()
        self.assertEqual((result["status"], result["reason"]), ("incompatible", "recovery_interrupted"))
        decoded = assert_proposal(self, result)
        with open(path, "rb") as handle:
            self.assertEqual(decoded["prior_journal_sha256"], hashlib.sha256(handle.read()).hexdigest())
        self.assertNotEqual(result["recovery"]["operation_id"], OPERATION)
        self.assertEqual(result["recovery"]["phase"], "proposed")
        self.assertIs(result["recovery"]["resumable"], False)

    def test_no_prior_journal_is_null_and_a_temporary_alone_is_not_a_prior_journal(self):
        self.stray()
        self.assertIsNone(assert_proposal(self, self.inspect())["prior_journal_sha256"])
        self.unfinished_journal("completed", name=JOURNAL + ".pending")
        self.assertIsNone(assert_proposal(self, self.inspect())["prior_journal_sha256"])

    def test_the_lock_identity_is_the_stable_lock_files_identity_or_null(self):
        self.stray()
        self.assertIsNone(assert_proposal(self, self.inspect())["lock_identity"])
        path = self.write("store.lock", b"")
        info = os.lstat(path)
        decoded = assert_proposal(self, self.inspect())
        self.assertEqual(decoded["lock_identity"], [info.st_dev, info.st_ino])
        legacy = self.write(".harflow-store.lock", b"different lock")
        legacy_info = os.lstat(legacy)
        self.assertNotEqual((info.st_dev, info.st_ino), (legacy_info.st_dev, legacy_info.st_ino))
        self.assertEqual(assert_proposal(self, self.inspect())["lock_identity"], [info.st_dev, info.st_ino])

    def test_the_root_identity_is_the_roots_device_and_inode(self):
        self.stray()
        info = os.lstat(self.state)
        self.assertEqual(assert_proposal(self, self.inspect())["root_identity"], [info.st_dev, info.st_ino])

    def test_each_inspection_proposes_a_fresh_operation_over_the_same_inventory(self):
        self.stray()
        first, second = self.inspect(), self.inspect()
        self.assertNotEqual(first["recovery"]["operation_id"], second["recovery"]["operation_id"])
        self.assertNotEqual(first["recovery"]["confirmation_token"], second["recovery"]["confirmation_token"])
        one = assert_proposal(self, first)
        two = assert_proposal(self, second)
        self.assertEqual(one["inventory_sha256"], two["inventory_sha256"])
        self.assertEqual(one["root_identity"], two["root_identity"])

    def test_the_inventory_digest_moves_with_the_tree(self):
        self.stray()
        base = assert_proposal(self, self.inspect())["inventory_sha256"]
        os.utime(os.path.join(self.state, "stray"), (1_000_000, 1_000_000))
        touched = assert_proposal(self, self.inspect())["inventory_sha256"]
        self.assertNotEqual(base, touched)
        self.write("added", b"y")
        self.assertNotEqual(touched, assert_proposal(self, self.inspect())["inventory_sha256"])

    def test_excluded_names_do_not_change_the_inventory_digest(self):
        self.stray()
        bare = assert_proposal(self, self.inspect())["inventory_sha256"]
        for name in (".harflow-store.lock", "store.lock", "io.lock", "store.recovery.tmp",
                     "store.sqlite3-shm"):
            self.write(name, b"x")
        # A pending record is also excluded and moves the verdict only.
        self.write("store.recovery", f"harflow-recovery-1\n{OPERATION}\npending\n")
        result = self.inspect()
        self.assertEqual(result["reason"], "recovery_interrupted")
        decoded = assert_proposal(self, result, digest=False)
        self.assertEqual(decoded["inventory_sha256"], bare)
        # The reference, told to leave the same names out, computes the same digest.
        if reference_available():
            self.assertEqual(decoded["inventory_sha256"], reference_inventory_digest(self.state, drop=EXTRA_EXCLUDED))

    def test_the_names_outside_the_exclusions_are_inventoried(self):
        self.stray()
        bare = assert_proposal(self, self.inspect())["inventory_sha256"]
        self.write("sub/store.lock", b"x")
        self.assertNotEqual(bare, assert_proposal(self, self.inspect())["inventory_sha256"])


class TokenTests(ProposalCase):
    def test_token_round_trips_through_the_cli_output(self):
        self.stray()
        result = self.inspect()
        token = result["recovery"]["confirmation_token"]
        self.assertTrue(token.isascii())
        self.assertNotIn("=", token)
        self.assertLessEqual(len(token), 8192)
        decoded = local_decode(token)
        self.assertEqual(local_encode(decoded), token)
        self.assertEqual(decoded["operation_id"], result["recovery"]["operation_id"])
        if reference_available():
            self.assertEqual(reference_decode(token), {"ok": decoded})
            self.assertEqual(reference_encode(decoded), token)

    def test_token_round_trips_canonical_with_ensure_ascii_escaping(self):
        # A state root and entry names outside ASCII (and one with a newline and quotes):
        # the reference decodes the token and recomputes the digest byte for byte.
        root = os.path.join(self.base, "état-中文-\U0001f600\"q\\")
        os.makedirs(root)
        for name in ("café", "中文", "\U0001f600", "new\nline", "quo\"te", "back\\slash", "\u007f"):
            self.write(name, b"x", root=root)
        result = self.inspect(root)
        self.assertEqual(result["state_root"], root)
        decoded = assert_proposal(self, result, root=root)
        self.assertEqual(decoded["state_root"], root)
        self.assertEqual(result["recovery"]["backup_path"], f"{root}.harflow-backup-{result['recovery']['operation_id']}")
        # The token's JSON holds only ASCII escapes, as the reference's default writes it.
        import base64
        raw = base64.urlsafe_b64decode(result["recovery"]["confirmation_token"] + "==").decode("ascii")
        self.assertIn("\\u00e9tat-\\u4e2d\\u6587-\\ud83d\\ude00", raw)

    def test_the_plain_output_carries_the_same_recovery_object_shape(self):
        self.stray()
        plain = self.ctl("state-inspect")
        self.assertEqual(plain.returncode, 0)
        document = json.loads(plain.stdout)
        structured = self.inspect()
        self.assertEqual(set(document), set(structured))
        self.assertEqual(set(document["recovery"]), set(structured["recovery"]))
        assert_proposal(self, document)

    def test_the_token_is_bound_to_this_engine_and_root(self):
        self.stray()
        result = self.inspect()
        decoded = local_decode(result["recovery"]["confirmation_token"])
        self.assertEqual(decoded["engine_version"], result["engine_version"])
        self.assertEqual(decoded["state_root"], self.state)

    def test_the_specimen_recovery_object_has_the_documented_keys(self):
        from ctl_support import fixture

        specimen = fixture("ctl-state-inspect.json")["result"]["recovery"]
        self.stray()
        self.assertEqual(set(self.inspect()["recovery"]), set(specimen))


if __name__ == "__main__":
    unittest.main()

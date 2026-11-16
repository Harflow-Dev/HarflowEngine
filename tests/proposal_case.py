"""Shared case class, imports and constants of the `state-inspect` proposal tests."""


import hashlib
import json
import os
import unittest  # noqa: F401

import ctl_extra
import ctl_rows
from ctl_rows import JOURNAL, MARKER, OPERATION
from ctl_support import CtlCase, journal, python_marker, rust_root, snapshot
from ctl_table import ROWS
from proposal_support import (
    EXTRA_EXCLUDED, assert_no_proposal, assert_proposal, reference_available, reference_decode,
    reference_encode, reference_inventory_digest, local_decode, local_encode,
)

OTHER_OPERATION = "fedcba9876543210fedcba9876543210"
UNFINISHED = ("planned", "prepared", "backed_up", "committed")
GENERATION = "a" * 32
RESULT_KEYS = {
    "cooperative_ownership_only", "engine_version", "message", "reason",
    "recovery_supported", "state_root", "status",
}
# Rows of the classification table that receive a proposal under plan 3.3. Every other row
# is ready or missing, carries Python state, has an unsafe root, a non-regular marker, lock
# or journal, an unreadable or unqualified journal, or an unfinished journal that does not
# hold a proposal of this engine.
PROPOSED_ROWS = {
    "legacy directory with content", "stray file", "stray directory",
    "more entries than the scan bound", "foreign product marker", "foreign format marker",
    "invalid json marker", "array marker", "binary marker", "oversize marker",
    "rust future identity", "rust older identity", "rust malformed identity",
    "rust identity without database", "rust foreign database", "rust future database",
    "rust file that is not a database", "rust truncated database header",
}
# Rows whose reserved file is a symbolic link: its proposal is covered by the walk tests.
SUSPECT_ROWS = {"identity is a symlink", "database is a symlink"}


class ProposalCase(CtlCase):
    def engine_version(self):
        return self.inspect(os.path.join(self.base, "absent"))["engine_version"]

    def stray(self, root=None):
        self.write("stray", b"x", root=root)

    def unfinished_journal(self, phase, version=None, name=JOURNAL, operation=OPERATION):
        """A well-formed journal whose proposal has every field, written into a stray root."""
        self.stray()
        info = os.lstat(self.state)
        identity = [info.st_dev, info.st_ino]
        proposal = {
            "state_root": self.state,
            "engine_version": self.engine_version() if version is None else version,
            "root_identity": identity,
            "lock_identity": None,
            "inventory_sha256": "c" * 64,
            "operation_id": operation,
            "prior_journal_sha256": None,
        }
        self.write(name, journal(self.state, phase, operation, proposal=proposal, root_identity=identity))
        return proposal

    def each(self, cases, check):
        for label, value in cases:
            with self.subTest(case=label):
                self.setUp()
                try:
                    check(value)
                finally:
                    self.cleanup()

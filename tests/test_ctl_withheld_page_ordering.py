"""Ordering of the withheld page and the marker refusal on the plain `task-list-page`.

The reference (HarflowMCP a426c5a, C1 section 7 and section 4.5) returns the withheld page from the
blocked catalog before the caller check, so a marker caller gets the refusal on a healthy root and
the withheld page on a root with one damaged task document.
"""

import json

import seed_support as seeds
from reads_support import ReadsCase
from seed_support import A, B

MARKER = {"HARFLOW_TASK_ID": "x"}
REFUSAL = "Task listing is refused: the caller is an agent whose workflow ownership cannot be resolved"
REASON = "Metadata or caller identity requires direct inspection"
WITHHELD = {
    "authority_incomplete": True,
    "bootstrap_pending": False,
    "catalog_state": {"reason": REASON, "source": "task_catalog", "status": "blocked"},
    "counts_complete": False,
    "has_more": False,
    "history_incomplete": True,
    "items": [],
    "next_cursor": None,
    "note": REASON,
    "related_headers": [],
}


class MarkerCallerDamageDisclosureTests(ReadsCase):
    def page(self):
        done = self.read("task-list-page", "--json", env=MARKER)
        document = json.loads(done.stdout)
        self.assertIsInstance(document.pop("product_version"), str)
        return done, document

    def test_a_marker_caller_learns_one_bit_whether_any_task_document_is_damaged(self):
        self.init(seeds.task(A), seeds.task(B))
        healthy, document = self.page()
        self.assertEqual(healthy.returncode, 1)
        self.assertEqual(healthy.stderr, "harflow-ctl: error: %s\n" % REFUSAL)
        self.assertEqual(document, {
            "error": {"code": "invalid_params", "message": REFUSAL},
            "format": "harflow.ctl", "product": "harflow", "v": 1,
        })

        self.seed("damage_task", {"task_id": B})
        damaged, document = self.page()
        self.assertEqual((damaged.returncode, damaged.stderr), (0, ""))
        self.assertEqual(document, {
            "format": "harflow.ctl", "product": "harflow", "v": 1, "result": WITHHELD,
        })
        self.assertTrue(document["result"]["authority_incomplete"])
        self.assertTrue(document["result"]["history_incomplete"])
        self.assertEqual(document["result"]["items"], [])
        self.assertNotIn(A, damaged.stdout)
        self.assertNotIn(B, damaged.stdout)

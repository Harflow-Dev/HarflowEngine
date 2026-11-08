"""Ledger item 117: `recovered` and `observation_mode` agree in every task reader.

Every projection branch of the table is seeded in one world, and each task is read through
`status`, `status --brief`, `status-batch`, `list` and `task-list-page`. The five readers must
report the same two values for the same task.
"""

from reads_support import ReadsCase
from test_ctl_reads_projection import TABLE

KEYS = ("recovered", "observation_mode")

# Not reachable through the fixture port: a known terminal status (completed, cancelled,
# timed_out) with no exit code and a dead process. `add_live_caller` accepts only running,
# completed_observed and failed_unobserved, and a task without a process row is never dead.
ROWS = tuple((row[0], row[1]) for row in TABLE)
EXPECTED = {"recovered": True, "observation_mode": "unknown"}


class RecoveredAndObservationModeAgreementTests(ReadsCase):
    def seed_world(self):
        self.alive_pid = self.sleeper().pid
        self.init()
        seeded = []
        for number, row in enumerate(ROWS, start=1):
            how = row[1]
            ident = "%08d-0000-4000-8000-000000000000" % number
            if how[0] == "live":
                self.live(ident, pid=self.alive_pid, identity=how[2], status=how[1])
            else:
                running = how[1] == "running"
                stamp = "2026-01-%02dT10:00:00+00:00" % number
                self.add(ident, status=how[1], exit_code=how[2], started_at=stamp,
                         finished_at=None if running else stamp, output_drained=not running)
            seeded.append((ident, row[0]))
        return seeded

    def pick(self, task):
        return {key: task[key] for key in KEYS}

    def test_every_branch_reports_the_same_two_fields_in_every_reader(self):
        seeded = self.seed_world()
        idents = [ident for ident, _ in seeded]
        listed = {t["task_id"]: t for t in self.good("list")["tasks"]}
        paged = {t["task_id"]: t for t in self.good("task-list-page")["result"]["items"]}
        batched = {t["task_id"]: t for t in
                   self.good("status-batch", "--task-ids", ",".join(idents))["tasks"]}
        self.assertEqual(len(listed), len(idents))
        self.assertEqual(len(paged), len(idents))
        self.assertEqual(len(batched), len(idents))
        for ident, name in seeded:
            readers = {
                "status": self.good("status", ident)["task"],
                "brief": self.good("status", ident, "--brief")["task"],
                "status-batch": batched[ident],
                "list": listed[ident],
                "task-list-page": paged[ident],
            }
            # One projection feeds every reader, so agreement is structural; the values are the
            # fixed ones the engine reports until owner facts exist.
            for reader, task in readers.items():
                with self.subTest(row=name, reader=reader):
                    self.assertLessEqual(set(KEYS), set(task))
                    self.assertEqual(self.pick(task), EXPECTED)
                    self.assertEqual(self.pick(task), self.pick(readers["status"]))

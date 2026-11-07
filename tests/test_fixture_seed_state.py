"""What hf-fixture-seed stores: rows read back with Python's own sqlite3 from a private copy
of the root, and files read from the real file system."""

import datetime
import json
import os
import subprocess
import time
import unittest

from seed_support import (
    A, B, C, D, E, LONG_PROMPT, SeedCase, a_events, b_events, sha256, task, world,
)

EPOCH = datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
FACTS = ("valid,backend,repo_path,session_id,status,exit_code,started_at,started_at_us,finished_at,"
         "finished_at_us,output_drained,title,model,continuation_of,chain_root")
PROMPT = "form,inline_text,preview,claimed_sha256,source_name,truncated,blob"
STAMP = "2026-01-10T10:04:00+00:00"
ITEM = {"artifact_id": "x1", "name": "a.txt", "target": "absent", "kind": "report", "note": "n",
        "registered_at": STAMP, "provenance_method": "verified_process_lineage",
        "observation": {"device": 1, "inode": 2, "size": 3, "mtime_ns": 4}}


def micros(text):
    return (datetime.datetime.fromisoformat(text) - EPOCH) // datetime.timedelta(microseconds=1)


class TaskRowTests(SeedCase):
    def fetch(self, columns, table, key, value):
        return self.rows(f"SELECT {columns} FROM {table} WHERE {key}=?", value)

    def test_add_task_stores_the_typed_facts_and_the_header_row(self):
        self.initialized(task(A, title="First task", group="g1", model="model-a"))
        self.assertEqual(self.fetch(FACTS, "task_facts", "id", A), [(
            1, "claude", "/workspace/repo", "session-aaaaaaaa", "completed", 0, "2026-01-10T10:00:00+00:00",
            micros("2026-01-10T10:00:00+00:00"), "2026-01-10T10:05:00+00:00", micros("2026-01-10T10:05:00+00:00"),
            1, "First task", "model-a", None, None)])
        self.assertEqual(self.fetch("stamp,session,active,recorded_status,exited,drained,owned", "task_headers", "id", A),
                         [(1768039200000000, "session-aaaaaaaa", 0, "completed", 1, 1, 0)])

    def test_the_served_blobs_carry_every_spec_field_and_the_projection_fields(self):
        self.initialized(task(A, title="First task", group="g1", model="model-a"))
        header, status = self.fetch("header,status", "task_headers", "id", A)[0]
        self.assertEqual(json.loads(header), {
            "task_id": A, "backend": "claude", "repo_path": "/workspace/repo", "freedom": "write_in_repo",
            "session_id": "session-aaaaaaaa", "started_at": "2026-01-10T10:00:00+00:00", "title": "First task",
            "group": "g1", "model": "model-a", "continuation_of_task_id": None, "chain_root_task_id": None})
        self.assertEqual(json.loads(status), {
            "task_id": A, "status": "completed", "exit_code": 0, "finished_at": "2026-01-10T10:05:00+00:00",
            "output_drained": True, "recovered": True, "observation_mode": "unknown",
            "needs_reconciliation": False, "owner": None, "phase_slots": [], "process_identity_state": "dead"})

    def test_offsets_are_parsed_to_instants_and_the_original_text_is_echoed(self):
        self.initialized(task(A, started_at="2026-01-10T12:00:00.5+02:00", finished_at="2026-01-10T12:05:00+0200"))
        self.assertEqual(self.fetch("started_at,started_at_us,finished_at,finished_at_us", "task_facts", "id", A), [(
            "2026-01-10T12:00:00.5+02:00", 1768039200500000, "2026-01-10T12:05:00+0200", 1768039500000000)])
        self.assertEqual(self.fetch("stamp", "task_headers", "id", A), [(1768039200500000,)])

    def test_a_running_task_is_active_and_has_no_finish(self):
        self.initialized(task(A, status="running", exit_code=None, finished_at=None, output_drained=False))
        self.assertEqual(self.fetch("exit_code,finished_at,finished_at_us,output_drained", "task_facts", "id", A),
                         [(None, None, None, 0)])
        self.assertEqual(self.fetch("active,recorded_status,exited,drained", "task_headers", "id", A),
                         [(1, "running", 0, 0)])

    def test_a_continuation_sets_both_links_and_an_empty_one_sets_neither(self):
        self.initialized(task(C, continuation_of=A), task(B, continuation_of=""), task(D, continuation_of=None))
        self.assertEqual(self.fetch("continuation_of,chain_root", "task_facts", "id", C), [(A, A)])
        self.assertEqual(self.fetch("continuation_of,chain_root", "task_facts", "id", B), [(None, None)])
        self.assertEqual(self.fetch("continuation_of,chain_root", "task_facts", "id", D), [(None, None)])
        header = json.loads(self.fetch("header", "task_headers", "id", C)[0][0])
        self.assertEqual((header["continuation_of_task_id"], header["chain_root_task_id"]), (A, A))

    def test_real_workspace_creates_the_repository_under_the_resolved_parent(self):
        self.initialized()
        expected = os.path.join(self.base, "repos", A)
        self.assertEqual(self.ok("add_task", {"spec": task(A, real_workspace=True)}, expect=None), {"repo_path": expected})
        self.assertTrue(os.path.isdir(expected))
        self.assertEqual(self.fetch("repo_path", "task_facts", "id", A), [(expected,)])
        self.assertEqual(json.loads(self.fetch("header", "task_headers", "id", A)[0][0])["repo_path"], expected)
        other = self.ok("add_task", {"spec": task(B, real_workspace=True)}, expect=None)
        self.assertEqual(other, {"repo_path": os.path.join(self.base, "repos", B)})

    def test_prompt_stores_follow_the_port(self):
        self.initialized(
            task(A), task(D, prompt=LONG_PROMPT, prompt_store="sidecar_verified"),
            task(E, prompt=LONG_PROMPT, prompt_store="sidecar_missing"),
            task(B, prompt=LONG_PROMPT, prompt_store="sidecar_mismatch"),
            task(C, prompt=LONG_PROMPT, prompt_store="sidecar_foreign_name"))
        true = sha256(LONG_PROMPT)
        absent = sha256(b"absent")
        preview = LONG_PROMPT[:2000]
        self.assertEqual(len(preview), 2000)
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", A), [("inline", "Summarize the repo.", None, None, None, 0, None)])
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", D),
                         [("sidecar", None, preview, true, f"{D}.prompt.{true}.txt", 1, LONG_PROMPT.encode())])
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", E),
                         [("sidecar", None, preview, absent, f"{E}.prompt.{absent}.txt", 1, None)])
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", B),
                         [("sidecar", None, preview, true, f"{B}.prompt.{true}.txt", 1, LONG_PROMPT.encode() + b"tampered")])
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", C),
                         [("sidecar", None, preview, None, "bad-name.txt", 1, None)])

    def test_tamper_prompt_appends_to_the_stored_blob_only(self):
        self.initialized(task(D, prompt=LONG_PROMPT, prompt_store="sidecar_verified"), task(A))
        before = self.fetch(PROMPT, "task_prompts", "task", D)[0]
        self.ok("tamper_prompt", {"task_id": D})
        after = self.fetch(PROMPT, "task_prompts", "task", D)[0]
        self.assertEqual(after[:6], before[:6])
        self.assertEqual(after[6], LONG_PROMPT.encode() + b"tampered")
        self.ok("tamper_prompt", {"task_id": A})
        self.assertEqual(self.fetch(PROMPT, "task_prompts", "task", A), [("inline", "Summarize the repo.", None, None, None, 0, None)])

    def test_damage_task_removes_only_the_header_and_process_rows(self):
        self.initialized(task(A), task(B))
        self.ok("add_live_caller", {"task_id": C, "pid": os.getpid(), "identity": "unknown", "status": "running", "marker": None},
                expect='{"supported":true}\n')
        self.ok("append_event", FIRST_A)
        self.ok("append_event", dict(FIRST_A, task_id=C))
        self.ok("damage_task", {"task_id": A})
        self.ok("damage_task", {"task_id": C})
        self.assertEqual(self.fetch(FACTS, "task_facts", "id", A), [(0,) + (None,) * 14])
        for task_id in (A, C):
            self.assertEqual(self.fetch("count(*)", "task_headers", "id", task_id), [(0,)])
            self.assertEqual(self.fetch("count(*)", "task_processes", "task", task_id), [(0,)])
            self.assertEqual(self.fetch("count(*)", "task_prompts", "task", task_id), [(1,)])
            self.assertEqual(self.fetch("count(*)", "event_streams", "task", task_id), [(1,)])
            self.assertEqual(self.fetch("count(*)", "event_records", "task", task_id), [(1,)])
        self.assertEqual(self.fetch("count(*)", "task_headers", "id", B), [(1,)])
        self.ok("damage_task", {"task_id": "12121212-1212-4212-8212-121212121212"})
        self.assertEqual(self.fetch("valid", "task_facts", "id", "12121212-1212-4212-8212-121212121212"), [(0,)])
        self.refused("add_task", {"spec": task(A)}, message="FixtureConflict")

    def test_a_damaged_task_refuses_appends_but_its_log_can_still_be_removed(self):
        self.initialized(task(A))
        self.ok("append_event", FIRST_A)
        self.ok("damage_task", {"task_id": A})
        before = self.digest()
        self.refused("append_event", dict(FIRST_A, seq=1), message="FixtureMissing")
        self.refused("append_malformed_line", {"task_id": A}, message="FixtureMissing")
        self.refused("append_partial_line", {"task_id": A}, message="FixtureMissing")
        self.assertEqual(self.digest(), before)
        self.ok("truncate_log", {"task_id": A, "keep_events": 0})
        self.ok("remove_log", {"task_id": A})
        self.assertEqual(self.fetch("count(*)", "event_streams", "task", A), [(0,)])
        self.assert_wal()

FIRST_A = {"task_id": A, "seq": 0, "kind": "task_started", "observed_at": "2026-01-10T10:00:00+00:00", "fields": {"backend": "claude"}}


class LiveCallerTests(SeedCase):
    def caller(self, task_id, identity, status="running"):
        return {"task_id": task_id, "pid": os.getpid(), "identity": identity, "status": status, "marker": None}

    def process(self, task_id):
        return self.rows("SELECT pid,start_seconds,start_micros,boot_uuid,pgid FROM task_processes WHERE task=?", task_id)[0]

    def test_alive_records_the_real_start_identity_of_the_pid(self):
        self.initialized()
        self.ok("add_live_caller", self.caller(A, "alive"), expect='{"supported":true}\n')
        pid, seconds, fraction, boot, group = self.process(A)
        started = subprocess.run(["/bin/ps", "-o", "lstart=", "-p", str(os.getpid())], capture_output=True, text=True,
                                 env={"LC_ALL": "C", "PATH": "/usr/bin:/bin"}).stdout.strip()
        expected = int(time.mktime(time.strptime(" ".join(started.split()), "%a %b %d %H:%M:%S %Y")))
        boot_session = subprocess.check_output(["/usr/sbin/sysctl", "-n", "kern.bootsessionuuid"], text=True).strip().lower()
        self.assertEqual((pid, seconds, boot, group), (os.getpid(), expected, boot_session, None))
        self.assertTrue(0 <= fraction < 1_000_000, fraction)

    def test_reused_differs_from_alive_and_unknown_has_no_start(self):
        self.initialized()
        for task_id, identity in ((A, "alive"), (B, "reused"), (C, "unknown")):
            self.ok("add_live_caller", self.caller(task_id, identity), expect='{"supported":true}\n')
        alive, reused, unknown = self.process(A), self.process(B), self.process(C)
        self.assertEqual(reused[0], os.getpid())
        self.assertEqual(reused[3], alive[3])
        self.assertNotEqual(reused[1:3], alive[1:3])
        self.assertEqual(unknown, (os.getpid(), None, None, None, None))
        states = {t: json.loads(self.rows("SELECT status FROM task_headers WHERE id=?", t)[0][0])["process_identity_state"] for t in (A, B, C)}
        self.assertEqual(states, {A: "alive", B: "dead", C: "uncertain"})

    def test_the_three_statuses_set_the_facts_as_the_port_does(self):
        self.initialized()
        for task_id, status in ((A, "running"), (B, "completed_observed"), (C, "failed_unobserved")):
            self.ok("add_live_caller", self.caller(task_id, "unknown", status), expect='{"supported":true}\n')
        columns = "status,exit_code,started_at,finished_at,output_drained,backend,repo_path,session_id"
        self.assertEqual(self.rows(f"SELECT {columns} FROM task_facts WHERE id=?", A), [(
            "running", None, "2026-11-07T00:00:00+00:00", None, 0, "claude", "/workspace/repo", "session-aaaaaaaa")])
        self.assertEqual(self.rows(f"SELECT {columns} FROM task_facts WHERE id=?", B), [(
            "completed", 0, "2026-11-07T00:00:00+00:00", "2026-11-07T00:01:00+00:00", 1, "claude", "/workspace/repo", "session-bbbbbbbb")])
        self.assertEqual(self.rows(f"SELECT {columns} FROM task_facts WHERE id=?", C), [(
            "failed", None, "2026-11-07T00:00:00+00:00", "2026-11-07T00:01:00+00:00", 0, "claude", "/workspace/repo", "session-cccccccc")])
        self.assertEqual(self.rows("SELECT active FROM task_headers WHERE id=?", A), [(1,)])
        self.assertEqual(self.rows("SELECT active,exited FROM task_headers WHERE id=?", C), [(0, 0)])


class EventRowTests(SeedCase):
    def records(self, task_id):
        return self.rows("SELECT start,end,class,seq,kind,body FROM event_records WHERE task=? ORDER BY start", task_id)

    def stream(self, task_id):
        return self.rows("SELECT complete_end,partial_tail,available,capture,drained,eof_seq,grace,task_status FROM event_streams WHERE task=?", task_id)

    def test_an_event_line_is_stored_normalized_with_its_flattened_fields(self):
        self.initialized(task(A))
        row = {"task_id": A, "seq": 4, "kind": "tool_call", "observed_at": "2026-01-10T10:03:00+00:00",
               "fields": {"call_id": "call-1", "name": "Read", "input": {"path": "README.md"}}}
        self.ok("append_event", row)
        (start, end, klass, seq, kind, body), = self.records(A)
        self.assertEqual((start, klass, seq, kind, end - start), (0, 0, 4, "tool_call", len(body) + 1))
        self.assertEqual(json.loads(body), {
            "product": "harflow", "format": "harflow.event", "v": 1, "seq": 4, "kind": "tool_call",
            "observed_at": "2026-01-10T10:03:00+00:00", "task_id": A, "call_id": "call-1", "name": "Read",
            "input": {"path": "README.md"}})

    def test_stream_facts_come_from_the_task_with_capture_unknown_unless_terminal_with_an_exit(self):
        self.initialized(task(A), task(B, status="failed", exit_code=None, output_drained=False),
                         task(C, status="running", exit_code=None, finished_at=None, output_drained=False))
        for task_id in (A, B, C):
            self.ok("append_event", dict(FIRST_A, task_id=task_id))
        end = self.records(A)[0][1]
        self.assertEqual(self.stream(A), [(end, 0, 1, 2, 1, None, 0, "completed")])
        self.assertEqual(self.stream(B), [(end, 0, 1, 3, 0, None, 0, "failed")])
        self.assertEqual(self.stream(C), [(end, 0, 1, 3, 0, None, 0, "running")])

    def test_malformed_and_partial_lines_have_the_port_byte_lengths(self):
        self.initialized(task(A))
        self.ok("append_event", FIRST_A)
        first = self.records(A)[0][1]
        self.ok("append_malformed_line", {"task_id": A})
        self.assertEqual(self.records(A)[1], (first, first + len(b"not json\n"), 2, None, None, b""))
        self.ok("append_partial_line", {"task_id": A})
        self.assertEqual(self.stream(A)[0][:2], (first + 9, len(b'{"partial":')))
        self.assertEqual(len(self.records(A)), 2)

    def test_a_partial_then_an_event_becomes_one_malformed_record(self):
        self.initialized(task(A))
        self.ok("append_partial_line", {"task_id": A})
        self.ok("append_event", FIRST_A)
        (start, end, klass, seq, kind, body), = self.records(A)
        self.assertEqual((start, klass, seq, kind, body), (0, 2, None, None, b""))
        line = {"product": "harflow", "format": "harflow.event", "v": 1, "seq": 0, "kind": "task_started",
                "observed_at": "2026-01-10T10:00:00+00:00", "task_id": A, "backend": "claude"}
        joined = len(b'{"partial":') + len(json.dumps(line, separators=(",", ":")).encode()) + 1
        self.assertEqual(end, joined)
        self.assertEqual(self.stream(A)[0][:2], (end, 0))

    def test_truncate_keeps_physical_lines_and_counts_an_unterminated_one(self):
        for keep, records, tail in ((0, 0, 0), (1, 1, 0), (2, 2, 0), (3, 3, 0), (4, 3, 11), (9, 3, 11)):
            self.initialized(task(A))
            self.ok("append_event", FIRST_A)
            self.ok("append_event", dict(FIRST_A, seq=1, kind="assistant_text"))
            self.ok("append_malformed_line", {"task_id": A})
            self.ok("append_partial_line", {"task_id": A})
            self.ok("truncate_log", {"task_id": A, "keep_events": keep})
            self.assertEqual((len(self.records(A)), self.stream(A)[0][1]), (records, tail), keep)
            self.ok("remove_root")

    def test_remove_log_deletes_the_stream_and_its_records_only(self):
        self.initialized(task(A), task(B))
        for task_id in (A, B):
            self.ok("append_event", dict(FIRST_A, task_id=task_id))
        self.ok("remove_log", {"task_id": A})
        self.assertEqual((self.records(A), self.stream(A)), ([], []))
        self.assertEqual(len(self.records(B)), 1)
        self.assertEqual(len(self.stream(B)), 1)


class ArtifactTests(SeedCase):
    def manifest(self, task_id):
        return self.rows("SELECT header_valid,length(revision) FROM artifact_manifests WHERE task=?", task_id)

    def items(self, task_id):
        return self.rows("SELECT ordinal,artifact_id,path,kind,note,registered_at,provenance_method,device,inode,size,mtime_ns FROM artifact_items WHERE task=? ORDER BY ordinal", task_id)

    def test_absent_targets_register_the_given_observation_under_the_repository_out_directory(self):
        self.initialized(task(A))
        second = dict(ITEM, artifact_id="x2", name="b.txt", observation={"device": 5, "inode": 6, "size": 7, "mtime_ns": 8})
        self.ok("write_artifacts", {"task_id": A, "items": [ITEM, second]},
                expect='{"paths":["/workspace/repo/out/a.txt","/workspace/repo/out/b.txt"]}\n')
        self.assertEqual(self.items(A), [
            (0, "x1", "/workspace/repo/out/a.txt", "report", "n", STAMP, "verified_process_lineage", "1", "2", 3, "4"),
            (1, "x2", "/workspace/repo/out/b.txt", "report", "n", STAMP, "verified_process_lineage", "5", "6", 7, "8")])
        self.assertEqual(self.manifest(A), [(1, 32)])
        self.assertFalse(os.path.exists("/workspace/repo/out/a.txt"))

    def test_a_present_target_is_created_and_its_real_stat_recorded(self):
        self.initialized()
        repo = self.ok("add_task", {"spec": task(A, real_workspace=True)}, expect=None)["repo_path"]
        present = {k: v for k, v in ITEM.items() if k != "observation"}
        present["target"] = "present"
        self.ok("write_artifacts", {"task_id": A, "items": [present]}, expect=None)
        path = os.path.join(repo, "out", "a.txt")
        with open(path, "rb") as stream:
            self.assertEqual(stream.read(), b"artifact bytes")
        info = os.stat(path)
        self.assertEqual(self.items(A)[0][7:], (str(info.st_dev), str(info.st_ino), 14, str(info.st_mtime_ns)))

    def test_header_valid_defaults_to_true_and_can_be_false(self):
        self.initialized(task(A))
        self.ok("write_artifacts", {"task_id": A, "items": []}, expect='{"paths":[]}\n')
        self.assertEqual(self.manifest(A), [(1, 32)])
        self.ok("write_artifacts", {"task_id": A, "items": [], "header_valid": False}, expect='{"paths":[]}\n')
        self.assertEqual((self.manifest(A), self.items(A)), ([(0, 32)], []))
        self.refused("write_artifacts", {"task_id": A, "items": [], "header_valid": None}, message="a boolean")
        self.refused("write_artifacts", {"task_id": A, "items": [], "header_valid": "no"}, message="a boolean")

    def test_a_second_write_replaces_the_manifest_and_changes_its_revision(self):
        self.initialized(task(A))
        self.ok("write_artifacts", {"task_id": A, "items": [ITEM, dict(ITEM, artifact_id="x2", name="b.txt")]}, expect=None)
        first = self.rows("SELECT revision FROM artifact_manifests WHERE task=?", A)
        self.ok("write_artifacts", {"task_id": A, "items": [ITEM]}, expect=None)
        self.assertEqual(len(self.items(A)), 1)
        self.assertNotEqual(self.rows("SELECT revision FROM artifact_manifests WHERE task=?", A), first)

    def test_unsafe_names_and_unknown_tasks_are_refused(self):
        self.initialized(task(A))
        for name in ("", ".", "..", "a/b", "../escape"):
            self.refused("write_artifacts", {"task_id": A, "items": [dict(ITEM, name=name)]}, message="one path component")
            self.refused("change_artifact_target", {"task_id": A, "name": name}, message="one path component")
        self.refused("write_artifacts", {"task_id": B, "items": []})
        self.refused("change_artifact_target", {"task_id": B, "name": "a.txt"})
        self.refused("write_artifacts", {"task_id": A, "items": [dict(ITEM, target="elsewhere")]}, message="unknown artifact target")

    def test_changing_a_target_edits_the_file_outside_the_root_and_leaves_the_digest_intact(self):
        self.initialized()
        repo = self.ok("add_task", {"spec": task(A, real_workspace=True)}, expect=None)["repo_path"]
        present = {k: v for k, v in ITEM.items() if k != "observation"}
        present["target"] = "present"
        self.ok("write_artifacts", {"task_id": A, "items": [present]}, expect=None)
        path = os.path.join(repo, "out", "a.txt")
        before = os.stat(path)
        digest = self.digest()
        self.ok("change_artifact_target", {"task_id": A, "name": "a.txt"})
        with open(path, "rb") as stream:
            self.assertEqual(stream.read(), b"artifact bytes changed")
        self.assertEqual(os.stat(path).st_size, before.st_size + len(b" changed"))
        self.assertEqual(self.digest(), digest)
        self.assertEqual(self.items(A)[0][9], 14, "the recorded size is untouched")
        self.assert_wal()


class WorldTests(SeedCase):
    def test_seed_world_stores_every_task_and_event_of_the_world(self):
        self.ok("seed_world", {"world": world()})
        stamps = self.rows("SELECT id,stamp,recorded_status FROM task_headers ORDER BY stamp DESC")
        self.assertEqual(stamps, [
            (E, micros("2026-01-14T10:00:00+00:00"), "completed"), (D, micros("2026-01-13T10:00:00+00:00"), "completed"),
            (C, micros("2026-01-12T10:00:00+00:00"), "completed"), (B, micros("2026-01-11T10:00:00+00:00"), "failed"),
            (A, micros("2026-01-10T10:00:00+00:00"), "completed")])
        self.assertEqual(self.rows("SELECT task,count(*),min(seq),max(seq) FROM event_records GROUP BY task ORDER BY task"),
                         [(A, 8, 0, 7), (B, 3, 0, 2)])
        self.assertEqual(self.rows("SELECT seq,kind FROM event_records WHERE task=? ORDER BY start", A),
                         [(e["seq"], e["kind"]) for e in a_events()])
        self.assertEqual(self.rows("SELECT seq,kind FROM event_records WHERE task=? ORDER BY start", B),
                         [(e["seq"], e["kind"]) for e in b_events()])
        self.assertEqual(self.rows("SELECT count(*) FROM event_streams"), [(2,)])

    def test_a_world_event_for_an_unknown_task_or_a_bad_task_is_refused(self):
        bad = world()
        bad["events"]["12121212-1212-4212-8212-121212121212"] = a_events()
        self.refused("seed_world", {"world": bad}, message="FixtureMissing")
        self.ok("remove_root")
        bad = world()
        bad["tasks"][0]["started_at"] = "not a time"
        self.refused("seed_world", {"world": bad}, message="unparseable timestamp")


if __name__ == "__main__":
    unittest.main()

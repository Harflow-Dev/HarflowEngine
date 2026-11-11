"""The hf-fixture-seed process protocol: all 20 operations, stdout shapes, exit codes,
refusals, the digest and the `-wal` guard. Values stored in the root are in
test_fixture_seed_state.py."""

import json
import os
import unittest

from seed_support import A, B, LEGACY, SeedCase, task, world

OPERATIONS = (
    "initialize_empty", "remove_root", "make_foreign_root", "root_state", "seed_world", "add_task",
    "add_live_caller", "damage_task", "append_event", "append_malformed_line", "append_partial_line",
    "truncate_log", "remove_log", "tamper_prompt", "write_artifacts", "change_artifact_target",
    "authoritative_digest", "stop_chain", "add_launch", "add_invalid_launch_name",
)
LAUNCH = {
    "name": "11111111-1111-4111-8111-111111111111", "state": "waiting_for_resources", "generation": "c" * 32,
    "backend": "claude", "title": "t", "repo_path": "/r", "created_at": "2026-11-10T00:00:00+00:00",
    "not_started_reason": None, "lineage": {}, "resources": {"names": ["fixture"]}, "worktree": None, "refusal": None,
}
WRITERS = (
    ("add_task", {"spec": task(B)}),
    ("add_live_caller", {"task_id": B, "pid": os.getpid(), "identity": "unknown", "status": "running", "marker": None}),
    ("add_live_caller", {"task_id": B, "pid": os.getpid(), "identity": "legacy_no_markers", "status": "running", "marker": None}),
    ("damage_task", {"task_id": A}),
    ("append_event", {"task_id": A, "seq": 1, "kind": "usage", "observed_at": "2026-01-10T10:00:01+00:00", "fields": {}}),
    ("append_malformed_line", {"task_id": A}),
    ("append_partial_line", {"task_id": A}),
    ("truncate_log", {"task_id": A, "keep_events": 1}),
    ("remove_log", {"task_id": A}),
    ("tamper_prompt", {"task_id": A}),
    ("write_artifacts", {"task_id": A, "items": []}),
    ("change_artifact_target", {"task_id": A, "name": "a.txt"}),
)
FIRST = {"task_id": A, "seq": 0, "kind": "task_started", "observed_at": "2026-01-10T10:00:00+00:00", "fields": {}}


def compact(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False) + "\n"


class ShapeTests(SeedCase):
    def test_all_twenty_operations_answer_with_their_documented_shapes(self):
        repo = os.path.join(self.base, "repos", A)
        self.assertEqual(self.ok("root_state", expect='"missing"\n'), "missing")
        self.ok("make_foreign_root")
        self.assertEqual(self.ok("root_state", expect='"other"\n'), "other")
        self.assertRegex(self.digest(), "^[0-9a-f]{64}$")
        self.ok("remove_root")
        self.ok("initialize_empty")
        self.assertEqual(self.ok("root_state", expect='"initialized"\n'), "initialized")
        self.ok("add_task", {"spec": task(A, real_workspace=True)}, expect=compact({"repo_path": repo}))
        self.ok("add_live_caller", {"task_id": B, "pid": os.getpid(), "identity": "alive", "status": "running", "marker": None},
                expect='{"supported":true}\n')
        self.ok("add_task", {"spec": task("dddddddd-dddd-4ddd-8ddd-dddddddddddd")}, expect="{}\n")
        self.ok("damage_task", {"task_id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd"})
        self.ok("append_event", FIRST)
        self.ok("append_malformed_line", {"task_id": A})
        self.ok("append_partial_line", {"task_id": A})
        self.ok("truncate_log", {"task_id": A, "keep_events": 2})
        self.ok("remove_log", {"task_id": A})
        self.ok("tamper_prompt", {"task_id": A})
        item = {"artifact_id": "x1", "name": "a.txt", "target": "present", "kind": "report", "note": "n",
                "registered_at": "2026-01-10T10:04:00+00:00", "provenance_method": "verified_process_lineage"}
        self.ok("write_artifacts", {"task_id": A, "items": [item]}, expect=compact({"paths": [repo + "/out/a.txt"]}))
        self.ok("change_artifact_target", {"task_id": A, "name": "a.txt"})
        self.ok("stop_chain", {"spec": {"chain_root": A, "mode": "marker",
                                        "attribution": {"kind": "valid", "cancelled_task_id": B}}})
        self.ok("add_launch", {"spec": LAUNCH})
        self.ok("add_invalid_launch_name", {"name": "bad name"})
        self.assertRegex(self.digest(), "^[0-9a-f]{64}$")
        self.ok("remove_root")
        self.ok("seed_world", {"world": world()})
        self.assertEqual(self.succeeded, set(OPERATIONS))

    def test_remove_root_tolerates_a_missing_root_and_removes_a_populated_one(self):
        self.ok("remove_root")
        self.initialized(task(A))
        self.ok("remove_root")
        self.assertFalse(os.path.lexists(self.state))

    def test_no_argument_operations_accept_empty_or_whitespace_stdin(self):
        self.ok("initialize_empty", stdin="")
        self.ok("root_state", stdin="  \n", expect='"initialized"\n')

    def test_root_state_distinguishes_missing_foreign_and_incomplete_roots(self):
        os.mkdir(self.state)
        self.ok("root_state", expect='"other"\n')
        os.rmdir(self.state)
        self.initialized()
        os.remove(os.path.join(self.state, "store.lock"))
        self.ok("root_state", expect='"other"\n')
        open(os.path.join(self.state, "store.lock"), "wb").close()
        self.ok("root_state", expect='"initialized"\n')
        marker = os.path.join(self.state, "store.identity")
        with open(marker) as stream:
            current = stream.read()
        with open(marker, "w") as stream:
            stream.write(current.replace("\n7\n", "\n6\n"))
        self.ok("root_state", expect='"other"\n')
        with open(marker, "w") as stream:
            stream.write("not a marker\n")
        self.ok("root_state", expect='"other"\n')
        os.remove(marker)
        self.ok("root_state", expect='"other"\n')

    def test_a_file_at_the_root_path_is_other(self):
        open(self.state, "wb").close()
        self.ok("root_state", expect='"other"\n')

    def test_make_foreign_root_writes_the_stranger_file_and_refuses_an_existing_root(self):
        self.ok("make_foreign_root")
        with open(os.path.join(self.state, "stranger.txt")) as stream:
            self.assertEqual(stream.read(), "not harflow")
        self.assertEqual(sorted(os.listdir(self.state)), ["stranger.txt"])
        before = self.digest()
        self.refused("make_foreign_root")
        self.assertEqual(self.digest(), before)

    def test_initialize_empty_and_seed_world_refuse_an_existing_root_and_leave_it_alone(self):
        for build in (lambda: self.initialized(task(A)), lambda: self.ok("make_foreign_root")):
            build()
            before = self.digest()
            for op, arguments in (("initialize_empty", {}), ("seed_world", {"world": world()})):
                message = self.refused(op, arguments, message="InvalidRoot")
                self.assertEqual(message.count("\n"), 1)
            self.assertEqual(self.digest(), before)
            self.ok("remove_root")

    def test_initialize_empty_leaves_a_regular_wal(self):
        self.ok("initialize_empty")
        self.assert_wal()

    def test_seed_world_leaves_a_regular_wal_and_an_initialized_root(self):
        self.ok("seed_world", {"world": world()})
        self.assert_wal()
        self.ok("root_state", expect='"initialized"\n')


class RefusalTests(SeedCase):
    def test_usage_errors_exit_one_with_a_prefixed_message(self):
        import subprocess
        from seed_support import ENVIRONMENT, binary
        for arguments in ([], ["root_state"], ["root_state", "--root"], ["root_state", "--state", self.state],
                          ["root_state", "--root", self.state, "extra"], ["root_state", "--root", "relative/path"]):
            done = subprocess.run([str(binary()), *arguments], input=b"{}", capture_output=True, env=dict(ENVIRONMENT))
            self.assertEqual((done.returncode, done.stdout), (1, b""), arguments)
            self.assertTrue(done.stderr.startswith(b"hf-fixture-seed: "), done.stderr)

    def test_unknown_operation_bad_json_and_oversized_requests_are_refused(self):
        self.refused("frobnicate", message="unknown operation")
        self.refused("root_state", stdin="{", message="not JSON")
        self.refused("root_state", stdin="[]", message="must be a JSON object")
        self.refused("root_state", stdin='"text"', message="must be a JSON object")
        self.refused("root_state", {"extra": 1}, message='unknown key "extra"')
        self.refused("add_task", stdin="", message='missing key "spec"')
        self.refused("root_state", stdin=" " * (4 * 1024 * 1024) + "{}", message="request too large")

    def test_every_required_key_of_every_operation_is_refused_when_missing(self):
        self.initialized(task(A))
        before = self.digest()
        item = {"artifact_id": "x", "name": "a.txt", "target": "absent", "kind": "k", "note": "n",
                "registered_at": "t", "provenance_method": "m", "observation": {"device": 1, "inode": 2, "size": 3, "mtime_ns": 4}}
        full = {
            "seed_world": {"world": world()}, "add_task": {"spec": task(B)},
            "add_live_caller": {"task_id": B, "pid": 4242, "identity": "unknown", "status": "running", "marker": None},
            "damage_task": {"task_id": A}, "append_event": FIRST, "append_malformed_line": {"task_id": A},
            "append_partial_line": {"task_id": A}, "truncate_log": {"task_id": A, "keep_events": 1},
            "remove_log": {"task_id": A}, "tamper_prompt": {"task_id": A},
            "write_artifacts": {"task_id": A, "items": [item]},
            "change_artifact_target": {"task_id": A, "name": "a.txt"},
        }
        for op, arguments in full.items():
            for key in arguments:
                partial = {k: v for k, v in arguments.items() if k != key}
                self.refused(op, partial, message=f'missing key "{key}"')
        self.assertEqual(self.digest(), before)

    def test_every_required_key_of_a_spec_an_item_and_an_observation_is_refused_when_missing(self):
        self.initialized(task(A))
        before = self.digest()
        for key in task(B):
            if key in ("title", "group", "model", "continuation_of", "prompt_store", "real_workspace"):
                continue
            spec = {k: v for k, v in task(B).items() if k != key}
            self.refused("add_task", {"spec": spec}, message=f'missing key "{key}"')
        item = {"artifact_id": "x", "name": "a.txt", "target": "absent", "kind": "k", "note": "n",
                "registered_at": "t", "provenance_method": "m", "observation": {"device": 1, "inode": 2, "size": 3, "mtime_ns": 4}}
        for key in [k for k in item if k != "observation"] + ["observation"]:
            self.refused("write_artifacts", {"task_id": A, "items": [{k: v for k, v in item.items() if k != key}]},
                         message=key if key != "observation" else "observation")
        for key in item["observation"]:
            observation = {k: v for k, v in item["observation"].items() if k != key}
            self.refused("write_artifacts", {"task_id": A, "items": [dict(item, observation=observation)]}, message=f'missing key "{key}"')
        self.assertEqual(self.digest(), before)

    def test_unknown_keys_and_wrong_types_are_refused(self):
        self.initialized(task(A))
        before = self.digest()
        self.refused("add_task", {"spec": dict(task(B), surprise=1)}, message='unknown key "surprise"')
        self.refused("add_task", {"spec": dict(task(B), output_drained="yes")}, message="a boolean")
        self.refused("add_task", {"spec": dict(task(B), status=3)}, message="a string")
        self.refused("add_task", {"spec": dict(task(B), exit_code="0")}, message="an integer or null")
        self.refused("add_task", {"spec": dict(task(B), prompt_store="elsewhere")}, message="unknown prompt_store")
        self.refused("add_task", {"spec": task("bad/id")}, message="invalid task id")
        self.refused("append_event", dict(FIRST, seq=-1), message="non-negative")
        self.refused("append_event", dict(FIRST, fields={"seq": 3}), message="reserved")
        self.refused("truncate_log", {"task_id": A, "keep_events": "1"}, message="non-negative")
        self.refused("add_live_caller", {"task_id": B, "pid": 4242, "identity": "unknown", "status": "paused", "marker": None},
                     message="unknown caller status")
        self.refused("add_live_caller", {"task_id": B, "pid": 2 ** 40, "identity": "unknown", "status": "running", "marker": None},
                     message="pid out of range")
        self.assertEqual(self.digest(), before)

    def test_unparseable_and_naive_timestamps_are_refused(self):
        self.initialized()
        for stamp in ("2026-01-10T10:00:00", "yesterday", "2026-02-30T10:00:00+00:00", ""):
            self.refused("add_task", {"spec": task(B, started_at=stamp)}, message="unparseable timestamp")
        self.refused("add_task", {"spec": task(B, finished_at="2026-01-10")}, message="unparseable timestamp")
        self.assertEqual(self.rows("SELECT count(*) FROM task_facts"), [(0,)])

    def test_a_duplicate_task_a_duplicate_live_caller_and_a_bad_pid_are_refused(self):
        self.initialized(task(A))
        before = self.digest()
        self.refused("add_task", {"spec": task(A, status="failed")}, message="FixtureConflict")
        self.refused("add_live_caller", {"task_id": A, "pid": os.getpid(), "identity": "unknown", "status": "running", "marker": None},
                     message="FixtureConflict")
        self.refused("add_live_caller", {"task_id": B, "pid": 1, "identity": "unknown", "status": "running", "marker": None})
        self.assertEqual(self.digest(), before)
        self.assert_wal()

    def test_log_operations_on_a_task_without_a_log_are_refused(self):
        self.initialized(task(A))
        self.refused("remove_log", {"task_id": A}, message="FixtureMissing")
        self.refused("truncate_log", {"task_id": A, "keep_events": 0}, message="FixtureMissing")
        self.refused("append_event", dict(FIRST, task_id=B), message="FixtureMissing")
        self.assert_wal()

    def test_a_missing_root_is_refused_by_the_store_operations_and_the_digest(self):
        for op, arguments in (("add_task", {"spec": task(A)}), ("damage_task", {"task_id": A}), ("authoritative_digest", {})):
            self.refused(op, arguments)
        self.assertFalse(os.path.lexists(self.state))

    def test_a_file_at_the_root_path_has_no_digest(self):
        open(self.state, "wb").close()
        self.refused("authoritative_digest", message="not a directory")


class WalGuardTests(SeedCase):
    def prepare(self):
        self.initialized(task(A))
        self.ok("append_event", FIRST)

    def test_every_mutating_operation_leaves_a_regular_wal(self):
        self.prepare()
        self.ok("add_task", {"spec": task("12121212-1212-4212-8212-121212121212", real_workspace=True)}, expect=None)
        present = {"artifact_id": "x", "name": "a.txt", "target": "present", "kind": "k", "note": "n",
                   "registered_at": "t", "provenance_method": "m"}
        self.ok("write_artifacts", {"task_id": "12121212-1212-4212-8212-121212121212", "items": [present]}, expect=None)
        self.assert_wal()
        self.ok("change_artifact_target", {"task_id": "12121212-1212-4212-8212-121212121212", "name": "a.txt"})
        self.assert_wal()
        for op, arguments in (
            ("add_task", {"spec": task(B)}), ("append_event", dict(FIRST, seq=1)), ("append_malformed_line", {"task_id": A}),
            ("append_partial_line", {"task_id": A}), ("truncate_log", {"task_id": A, "keep_events": 2}),
            ("tamper_prompt", {"task_id": A}), ("write_artifacts", {"task_id": A, "items": []}),
            ("add_live_caller", {"task_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc", "pid": os.getpid(), "identity": "reused", "status": "running", "marker": None}),
            ("remove_log", {"task_id": A}), ("damage_task", {"task_id": B}),
        ):
            self.ok(op, arguments, expect=None)
            self.assert_wal()

    def test_a_missing_wal_before_the_call_is_refused_and_not_recreated(self):
        for op, arguments in WRITERS:
            self.prepare()
            os.remove(self.wal_path())
            before = self.digest()
            self.assertRegex(self.refused(op, arguments), "store.sqlite3-wal|WalMissing")
            self.assertFalse(os.path.lexists(self.wal_path()), op)
            self.assertEqual(self.digest(), before, op)
            self.ok("remove_root")

    def test_a_wal_that_is_a_directory_or_a_symlink_is_refused(self):
        for make in (lambda p: os.mkdir(p), lambda p: os.symlink(os.path.join(self.base, "elsewhere"), p)):
            self.prepare()
            open(os.path.join(self.base, "elsewhere"), "wb").close()
            os.remove(self.wal_path())
            make(self.wal_path())
            self.refused("append_partial_line", {"task_id": A}, message="store.sqlite3-wal")
            self.refused("add_task", {"spec": task(B)}, message="store.sqlite3-wal")
            self.ok("remove_root")

    def test_the_wal_is_never_checkpointed_into_the_main_file(self):
        self.prepare()
        self.assertGreater(os.path.getsize(self.wal_path()), 0)
        for _ in range(3):
            self.ok("append_partial_line", {"task_id": A})
        self.assertGreater(os.path.getsize(self.wal_path()), 0)


class DigestTests(SeedCase):
    def test_digest_changes_on_every_event_log_mutation(self):
        self.initialized(task(A))
        seen = [self.digest()]
        for op, arguments in (
            ("append_event", FIRST), ("append_partial_line", {"task_id": A}), ("append_malformed_line", {"task_id": A}),
            ("truncate_log", {"task_id": A, "keep_events": 1}), ("remove_log", {"task_id": A}),
        ):
            self.ok(op, arguments)
            seen.append(self.digest())
        self.assertEqual(len(set(seen)), len(seen), seen)

    def test_digest_is_stable_across_repeated_calls_and_reads_of_the_root(self):
        self.initialized(task(A))
        first = self.digest()
        self.assertEqual(self.digest(), first)
        self.ok("root_state", expect='"initialized"\n')
        self.assertEqual(self.digest(), first)

    def test_digest_ignores_the_lock_files_and_the_shared_memory_index_but_not_the_wal(self):
        self.initialized(task(A))
        before = self.digest()
        for name in ("store.lock", "io.lock", "store.sqlite3-shm"):
            with open(os.path.join(self.state, name), "ab") as stream:
                stream.write(b"changed")
        self.assertEqual(self.digest(), before)
        with open(self.wal_path(), "ab") as stream:
            stream.write(b"0")
        self.assertNotEqual(self.digest(), before)

    def test_digest_of_a_foreign_root_reacts_to_content_names_kinds_and_links(self):
        os.mkdir(self.state)
        def write(name, data):
            with open(os.path.join(self.state, name), "wb") as stream:
                stream.write(data)

        write("stranger.txt", b"one")
        digests = {"base": self.digest()}
        write("stranger.txt", b"two")
        digests["content"] = self.digest()
        write("stranger.txt", b"one")
        self.assertEqual(self.digest(), digests["base"])
        os.rename(os.path.join(self.state, "stranger.txt"), os.path.join(self.state, "renamed.txt"))
        digests["name"] = self.digest()
        os.rename(os.path.join(self.state, "renamed.txt"), os.path.join(self.state, "stranger.txt"))
        os.mkdir(os.path.join(self.state, "empty"))
        digests["directory"] = self.digest()
        os.symlink("target-a", os.path.join(self.state, "link"))
        digests["link"] = self.digest()
        os.remove(os.path.join(self.state, "link"))
        os.symlink("target-b", os.path.join(self.state, "link"))
        digests["retarget"] = self.digest()
        self.assertEqual(len(set(digests.values())), len(digests), digests)

    def test_digest_depends_on_content_not_on_creation_order_or_the_root_path(self):
        other = os.path.join(self.base, "other")
        for root, names in ((self.state, ("b", "a", "c")), (other, ("c", "a", "b"))):
            os.makedirs(os.path.join(root, "sub"))
            for name in names:
                with open(os.path.join(root, "sub", name), "wb") as stream:
                    stream.write(name.encode())
        self.assertEqual(self.digest(), self.digest(root=other))

    def test_a_root_holding_a_fifo_has_no_digest(self):
        os.mkdir(self.state)
        os.mkfifo(os.path.join(self.state, "pipe"))
        self.refused("authoritative_digest", message="unsupported file type")


class IdentityShapeTests(SeedCase):
    def caller(self, task_id, identity, status="running", marker=None):
        return {"task_id": task_id, "pid": os.getpid(), "identity": identity, "status": status, "marker": marker}

    def test_supported_is_an_explicit_boolean_for_every_state(self):
        self.initialized()
        for index, identity in enumerate(("alive", "reused", "unknown")):
            task_id = "%08d-0000-4000-8000-000000000000" % index
            result = self.ok("add_live_caller", self.caller(task_id, identity), expect=None)
            self.assertEqual(set(result), {"supported"})
            self.assertIs(result["supported"], True)
        for index, identity in enumerate(LEGACY):
            task_id = "%08d-1111-4111-8111-111111111111" % index
            marker = "marker" if identity in ("legacy_alive_with_markers", "legacy_markers_seen") else None
            done = self.call("add_live_caller", self.caller(task_id, identity, marker=marker))
            self.assertEqual((done.returncode, done.stdout, done.stderr), (0, b'{"supported":false}\n', b""), identity)
        self.assertIs(self.ok("add_live_caller", self.caller(A, "something_else"), expect=None)["supported"], False)

    def test_an_unsupported_state_creates_nothing(self):
        self.initialized(task(A))
        before = self.digest()
        for identity in LEGACY:
            self.ok("add_live_caller", self.caller(B, identity), expect='{"supported":false}\n')
            self.assertEqual(self.digest(), before, identity)
        self.assertEqual(self.rows("SELECT count(*) FROM task_processes"), [(0,)])
        self.assert_wal()


if __name__ == "__main__":
    unittest.main()

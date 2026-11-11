"""Black-box tests of the availability of artifact targets, against real files: what is observed,
where a reference may point, and that observing writes nothing.

The reference engine is artifacts.py `_availability` and `_target` of HarflowMCP a426c5a; its
authority subset is ported here (scratch, writable-root and grant sources have no facts yet).
"""

import os
import stat

import seed_support as seeds
from history_support import FORBIDDEN_NAMES, ArtifactCase, item
from seed_support import A

NAMESPACE_FILES = (".mcp.json", "opencode.json", "opencode.jsonc")
NAMESPACE_DIRECTORIES = (".opencode", ".gemini", ".antigravity", ".cursor", ".agents/skills")


class ObservationTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"))
        self.path = os.path.join(self.repo_path, "out", "a.txt")

    def test_an_untouched_file_is_available_with_its_registered_observation(self):
        entry = self.listed()["items"][0]
        info = os.stat(self.path)
        self.assertEqual(entry["availability"]["state"], "available")
        self.assertEqual(entry["registered_observation"], {
            "device": info.st_dev, "inode": info.st_ino, "size": info.st_size, "mtime_ns": info.st_mtime_ns})
        self.assertEqual(entry["path"], self.path)
        self.assertEqual(entry["provenance"], {"method": "verified_process_lineage", "task_id": A})

    def test_a_changed_size_or_mtime_is_changed(self):
        self.seed("change_artifact_target", {"task_id": A, "name": "a.txt"})
        self.assertEqual(self.outcome(), "changed")

    def test_the_same_size_with_another_mtime_is_changed(self):
        info = os.stat(self.path)
        os.utime(self.path, ns=(info.st_atime_ns, info.st_mtime_ns + 5_000_000_000))
        self.assertEqual(self.outcome(), "changed")

    def test_a_replaced_file_with_another_inode_is_changed(self):
        info = os.stat(self.path)
        os.remove(self.path)
        with open(self.path, "wb") as stream:
            stream.write(b"artifact bytes")
        os.utime(self.path, ns=(info.st_atime_ns, info.st_mtime_ns))
        self.assertEqual(self.outcome(), "changed")

    def test_a_removed_file_is_deleted(self):
        os.remove(self.path)
        self.assertEqual(self.outcome(), "deleted")

    def test_a_removed_parent_directory_is_deleted(self):
        os.remove(self.path)
        os.rmdir(os.path.dirname(self.path))
        self.assertEqual(self.outcome(), "deleted")

    def test_an_absent_target_registered_with_an_observation_is_deleted(self):
        other = dict(item("x2", "never.txt"), target="absent", observation={
            "device": 1, "inode": 2, "size": 3, "mtime_ns": 4})
        self.register(item("x1", "a.txt"), other)
        self.assertEqual(self.states(), {"x1": "available", "x2": "deleted"})

    def test_an_absent_target_under_a_missing_repository_is_deleted_not_unavailable(self):
        self.add("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", repo_path="/nonexistent-harflow-repo/work",
                 started_at="2026-01-11T10:00:00+00:00")
        gone = dict(item("x1", "a.txt"), target="absent", observation={"device": 1, "inode": 2, "size": 3, "mtime_ns": 4})
        self.register(gone, task_id="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        self.assertEqual(self.states(task_id="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"), {"x1": "deleted"})

    def test_a_present_target_under_a_missing_repository_cannot_be_seen_as_inside_it(self):
        base = os.path.join(self.base, "vanished")
        self.add("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", repo_path=base, started_at="2026-01-11T10:00:00+00:00")
        self.register(item("x1", "a.txt"), task_id="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        self.assertEqual(self.states(task_id="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"), {"x1": "available"})
        os.rename(base, base + "-moved")
        self.assertEqual(self.states(task_id="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"), {"x1": "deleted"})

    def test_the_observation_time_is_the_read_time(self):
        entry = self.listed()["items"][0]
        self.assertRegex(entry["availability"]["observed_at"], r"^\d{4}-\d\d-\d\dT")
        self.assertEqual(set(entry["availability"]), {"state", "observed_at"})


class SubstitutionTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"))
        self.out = os.path.join(self.repo_path, "out")
        self.path = os.path.join(self.out, "a.txt")

    def test_a_symlink_in_place_of_the_file_is_changed(self):
        elsewhere = self.file_at("other", "b.txt")
        os.remove(self.path)
        os.symlink(elsewhere, self.path)
        self.assertEqual(self.outcome(), "changed")

    def test_a_dangling_symlink_is_deleted(self):
        os.remove(self.path)
        os.symlink(os.path.join(self.repo_path, "nowhere"), self.path)
        self.assertEqual(self.outcome(), "deleted")

    def test_a_parent_directory_replaced_by_a_symlink_is_changed(self):
        moved = self.out + "-real"
        os.rename(self.out, moved)
        os.symlink(moved, self.out)
        self.assertEqual(self.outcome(), "changed")

    def test_a_parent_symlink_to_a_missing_directory_is_deleted(self):
        os.remove(self.path)
        os.rmdir(self.out)
        os.symlink(os.path.join(self.repo_path, "missing"), self.out)
        self.assertEqual(self.outcome(), "deleted")

    def test_a_registered_path_through_a_symlink_is_changed_even_when_the_file_is_untouched(self):
        link = os.path.join(self.repo_path, "link")
        os.symlink(self.out, link)
        self.move_to(os.path.join(link, "a.txt"))
        self.assertEqual(self.outcome(), "changed")

    def test_a_fifo_in_place_of_the_file_is_unavailable_and_never_blocks(self):
        os.remove(self.path)
        os.mkfifo(self.path)
        self.assertEqual(self.outcome(), "unavailable")
        self.assertTrue(stat.S_ISFIFO(os.lstat(self.path).st_mode))

    def test_a_directory_in_place_of_the_file_is_unavailable(self):
        os.remove(self.path)
        os.mkdir(self.path)
        self.assertEqual(self.outcome(), "unavailable")

    def test_an_unreadable_file_is_unavailable(self):
        if os.geteuid() == 0:
            self.skipTest("root reads every file")
        os.chmod(self.path, 0)
        self.assertEqual(self.outcome(), "unavailable")
        os.chmod(self.path, 0o600)
        self.assertEqual(self.outcome(), "available")

    def test_a_file_in_an_unsearchable_directory_is_not_reported_as_deleted(self):
        if os.geteuid() == 0:
            self.skipTest("root searches every directory")
        os.chmod(self.out, 0)
        self.assertEqual(self.outcome(), "unavailable")
        os.chmod(self.out, 0o700)

    def test_observing_leaves_the_files_as_they_were(self):
        before = (os.lstat(self.path), os.lstat(self.out), sorted(os.listdir(self.out)))
        self.states()
        after = (os.lstat(self.path), os.lstat(self.out), sorted(os.listdir(self.out)))
        self.assertEqual([i.st_mtime_ns for i in before[:2]], [i.st_mtime_ns for i in after[:2]])
        self.assertEqual(before[2], after[2])

    def test_the_path_is_not_followed_to_read_content(self):
        os.remove(self.path)
        os.mkfifo(self.path)
        self.states()
        self.assertEqual(os.listdir(self.out), ["a.txt"])


class AuthorityTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"), item("x2", "b.txt"))

    def check(self, path, expected, **options):
        self.move_to(path, "x2")
        self.assertEqual(self.states(**options)["x2"], expected, path)

    def test_a_target_outside_the_repository_is_unavailable(self):
        for directory in (self.base, self.home):
            outside = os.path.join(directory, "outside.txt")
            with open(outside, "wb") as stream:
                stream.write(b"x")
            with self.subTest(directory=os.path.basename(directory)):
                self.check(outside, "unavailable")

    def test_a_target_in_a_sibling_of_the_repository_sharing_its_prefix_is_unavailable(self):
        sibling = self.repo_path + "-twin"
        os.makedirs(sibling)
        with open(os.path.join(sibling, "t.txt"), "wb") as stream:
            stream.write(b"x")
        self.check(os.path.join(sibling, "t.txt"), "unavailable")

    def test_a_file_directly_in_the_repository_is_available(self):
        self.check(self.file_at("plain.txt"), "available")

    def test_a_file_named_like_each_forbidden_name_is_unavailable(self):
        for name in FORBIDDEN_NAMES:
            with self.subTest(name=name):
                self.check(self.file_at("out", name), "unavailable")

    def test_a_file_under_a_directory_named_like_each_forbidden_name_is_unavailable(self):
        for name in FORBIDDEN_NAMES:
            with self.subTest(name=name):
                self.check(self.file_at(name, "inner.txt"), "unavailable")
                self.check(self.file_at("deep", name, "more", "inner.txt"), "unavailable")

    def test_a_name_that_only_contains_a_forbidden_name_is_not_forbidden(self):
        for name in (".gitignore", ".github", "x.git", ".claude-notes", "config", ".harflowx"):
            with self.subTest(name=name):
                self.check(self.file_at("out", name), "available")

    def test_project_namespace_files_at_the_repository_root_are_unavailable(self):
        for name in NAMESPACE_FILES:
            with self.subTest(name=name):
                self.check(self.file_at(name), "unavailable")

    def test_project_namespace_directories_at_the_repository_root_are_unavailable(self):
        for name in NAMESPACE_DIRECTORIES:
            with self.subTest(name=name):
                self.check(self.file_at(name, "inner.txt"), "unavailable")
                self.check(self.file_at(name, "deeper", "inner.txt"), "unavailable")

    def test_the_mcp_json_of_a_subdirectory_is_not_a_namespace_as_in_the_reference(self):
        for parts in (("out", ".mcp.json"), ("sub", ".mcp.json"), ("sub", "opencode.json"), ("sub", ".opencode", "x"),
                      ("sub", ".agents", "skills", "x")):
            with self.subTest(parts=parts):
                self.check(self.file_at(*parts), "available")

    def test_a_repository_below_a_project_namespace_directory_is_unavailable(self):
        for name in NAMESPACE_DIRECTORIES + ("holder",):
            with self.subTest(name=name):
                nested = os.path.join(self.base, "below", name, "repo")
                os.makedirs(nested)
                self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (nested, A))
                with open(os.path.join(nested, "f.txt"), "wb"):
                    pass
                self.check(os.path.join(nested, "f.txt"), "available" if name == "holder" else "unavailable")

    def test_the_provenance_methods_this_engine_cannot_judge_are_unavailable(self):
        self.register(item("x1", "a.txt"), item("x2", "b.txt", method="engine_output"),
                      item("x3", "c.txt", method="observed_backend_plan"))
        self.assertEqual(self.states(), {"x1": "available", "x2": "unavailable", "x3": "unavailable"})

    def test_an_item_is_judged_by_its_own_target_not_its_neighbours(self):
        self.file_at("out", ".git")
        self.register(item("x1", "a.txt"), item("x2", ".git"), item("x3", "c.txt"))
        self.assertEqual(self.states(), {"x1": "available", "x2": "unavailable", "x3": "available"})

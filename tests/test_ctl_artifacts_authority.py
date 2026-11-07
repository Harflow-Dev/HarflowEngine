"""Black-box tests of the external configuration namespaces an artifact may not expose, the
environment they come from, and the spelling of paths on a case-insensitive volume."""

import os
import sys
import unittest

from history_support import PREDECESSOR_DIRECTORY, ArtifactCase, item
from seed_support import A

INVALID_VALUES = ("", "relative/path", "./relative")
HOME_NAMESPACES = (
    ".claude.json", ".gemini/x", ".cursor/x", ".agents/skills/x", ".vibe/x", ".codex/x", ".claude/x",
    ".local/share/opencode/x", ".cache/opencode/x", ".local/state/opencode/x", ".config/opencode/x",
)


class ExternalNamespaceTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"), item("x2", "b.txt"))

    def check(self, parts, expected, env=None, base=None):
        path = os.path.join(base or self.repo_path, *parts)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as stream:
            stream.write(b"x")
        self.move_to(path, "x2")
        self.assertEqual(self.states(env=env)["x2"], expected, (parts, env))

    def under_repo(self, name):
        return os.path.join(self.repo_path, name)

    def test_a_configured_backend_directory_inside_the_repository_is_unavailable(self):
        cases = (
            ("CLAUDE_CONFIG_DIR", "claudecfg", ("claudecfg", "a.txt")),
            ("CLAUDE_CONFIG_DIR", "claudecfg", ("claudecfg.json",)),
            ("CODEX_HOME", "codexhome", ("codexhome", "a.txt")),
            ("VIBE_HOME", "vibehome", ("vibehome", "a.txt")),
            ("XDG_CONFIG_HOME", "xc", ("xc", "opencode", "a.txt")),
            ("XDG_DATA_HOME", "xd", ("xd", "opencode", "a.txt")),
            ("XDG_CACHE_HOME", "xk", ("xk", "opencode", "a.txt")),
            ("XDG_STATE_HOME", "xs", ("xs", "opencode", "a.txt")),
            ("OPENCODE_CONFIG_DIR", "ocd", ("ocd", "a.txt")),
            ("OPENCODE_CONFIG", "oc.json", ("oc.json",)),
            ("OPENCODE_TUI_CONFIG", "tui.json", ("tui.json",)),
            ("OPENCODE_TEST_MANAGED_CONFIG_DIR", "managed", ("managed", "a.txt")),
        )
        for name, value, parts in cases:
            with self.subTest(variable=name, target="/".join(parts)):
                env = {name: self.under_repo(value)}
                self.check(("elsewhere", "a.txt"), "available", env)
                self.check(parts, "unavailable", env)

    def test_the_opencode_home_moves_its_default_locations(self):
        env = {"OPENCODE_TEST_HOME": self.under_repo("ohome")}
        self.check(("ohome", ".local", "share", "opencode", "a.txt"), "unavailable", env)
        self.check(("ohome", ".config", "opencode", "a.txt"), "unavailable", env)
        self.check(("ohome", "plain.txt"), "available", env)

    def test_a_home_prefixed_value_is_expanded(self):
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.home, A))
        env = {"CODEX_HOME": "~/codex-state"}
        self.check(("codex-state", "a.txt"), "unavailable", env, base=self.home)
        self.check(("elsewhere", "a.txt"), "available", env, base=self.home)

    def test_a_repository_that_is_the_home_directory_cannot_expose_backend_state(self):
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.home, A))
        for parts in HOME_NAMESPACES:
            with self.subTest(target=parts):
                self.check(tuple(parts.split("/")), "unavailable", base=self.home)
        self.check(("notes.txt",), "available", base=self.home)
        self.check(("projects", "app", "main.txt"), "available", base=self.home)

    def test_an_invalid_namespace_variable_fails_closed_for_every_target(self):
        for name in ("CLAUDE_CONFIG_DIR", "CODEX_HOME", "VIBE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
                     "XDG_CACHE_HOME", "XDG_STATE_HOME", "OPENCODE_CONFIG", "OPENCODE_CONFIG_DIR",
                     "OPENCODE_TUI_CONFIG", "OPENCODE_TEST_MANAGED_CONFIG_DIR", "OPENCODE_TEST_HOME"):
            for value in INVALID_VALUES:
                with self.subTest(variable=name, value=value):
                    self.check(("out", "a.txt"), "unavailable", {name: value})
        self.check(("out", "a.txt"), "available", {"CODEX_HOME": self.under_repo("c")})

    def test_the_selected_state_root_is_protected_when_the_repository_contains_it(self):
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.base, A))
        for name in ("store.identity", "store.sqlite3"):
            with self.subTest(name=name):
                path = os.path.join(self.state, name)
                self.move_to(path, "x2")
                self.assertEqual(self.states()["x2"], "unavailable")
        self.check(("work", "notes.txt"), "available", base=self.base)

    def test_a_default_state_directory_of_the_home_is_protected(self):
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.home, A))
        self.check((".harflow", "a.txt"), "unavailable", base=self.home)
        self.check((PREDECESSOR_DIRECTORY, "a.txt"), "unavailable", base=self.home)

    def test_a_symlink_that_leads_into_a_namespace_is_a_different_path_and_so_changed(self):
        target = os.path.join(self.repo_path, "claudecfg")
        os.makedirs(target)
        link = os.path.join(self.repo_path, "viewed")
        os.symlink(target, link)
        self.file_at("claudecfg", "a.txt")
        self.move_to(os.path.join(link, "a.txt"), "x2")
        self.assertEqual(self.states(env={"CLAUDE_CONFIG_DIR": target})["x2"], "changed")


@unittest.skipUnless(sys.platform == "darwin", "macOS volumes only")
class CaseSpellingTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"), item("x2", "b.txt"))
        if not os.path.exists(os.path.join(self.repo_path, "OUT")):
            self.skipTest("the volume is case-sensitive")

    def at(self, path, artifact_id="x1"):
        self.move_to(path, artifact_id)

    def test_a_path_registered_in_another_case_is_not_the_canonical_path_and_so_changed(self):
        self.at(os.path.join(self.repo_path, "out", "A.TXT"))
        self.assertEqual(self.states()["x1"], "changed")
        self.at(os.path.join(self.repo_path, "OUT", "a.txt"))
        self.assertEqual(self.states()["x1"], "changed")

    def test_an_alias_of_a_protected_name_never_reaches_the_authority_check(self):
        for spelled, real in ((".GIT", ".git"), (".Claude", ".claude"), (".HARFLOW", ".harflow")):
            with self.subTest(spelled=spelled):
                self.file_at("out", real)
                self.at(os.path.join(self.repo_path, "out", spelled))
                self.assertEqual(self.states()["x1"], "changed")
                self.at(os.path.join(self.repo_path, "out", real))
                self.assertEqual(self.states()["x1"], "unavailable")

    def test_a_repository_recorded_in_another_case_still_judges_the_canonical_target(self):
        alias = os.path.join(os.path.dirname(self.repo_path), os.path.basename(self.repo_path).upper())
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (alias, A))
        self.assertEqual(self.states()["x1"], "available")

    def test_a_project_namespace_on_disk_in_another_case_is_unavailable_in_its_real_spelling(self):
        """As in the reference: the namespace `<repo>/.mcp.json` is respelled as the disk has it."""
        for parts in ((".MCP.json",), (".Cursor", "x.txt"), ("OpenCode.JSON",), (".Agents", "Skills", "x.txt")):
            with self.subTest(parts=parts):
                self.at(self.file_at(*parts), "x2")
                self.assertEqual(self.states()["x2"], "unavailable")
        self.at(self.file_at("sub", ".MCP.json"), "x2")
        self.assertEqual(self.states()["x2"], "available", "only the repository root is a namespace")

    def test_a_home_spelled_in_another_case_still_protects_its_backend_state(self):
        """The reference rewrites every component of a namespace to the kernel's spelling before
        comparing (canonical_paths._chain), and so does this engine: with HOME spelled `HOME` for
        the directory `home`, ~/.claude.json stays unavailable."""
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (self.home, A))
        path = os.path.join(self.home, ".claude.json")
        with open(path, "wb") as stream:
            stream.write(b"{}")
        self.at(path)
        self.assertEqual(self.states()["x1"], "unavailable")
        alias = os.path.join(os.path.dirname(self.home), "HOME")
        self.assertEqual(self.states(env={"HOME": alias})["x1"], "unavailable")

"""Black-box tests of `~` in the configured backend paths, read as `Path(value).expanduser()` and
then required to be absolute, as the reference's `environment_path` does (config_namespaces.py of
HarflowMCP a426c5a). Defaults are not expanded."""

import getpass
import os

from history_support import ArtifactCase, item
from seed_support import A

ME = getpass.getuser()
# Computed with python3 3.13.5: `Path(v).expanduser()` then `is_absolute()` and a non-empty value.
# `~name` goes through the account database, an unknown account is a RuntimeError, so the
# reference refuses that artifact.
ACCEPTED = {
    "~": True,
    "~/x": True,
    "~ME/x": True,
    "~nobody/x": True,
    "~root/x": True,
    "~nosuchuser/x": False,
    "~~": False,
    "": False,
    "relative/path": False,
}
VARIABLES = ("CODEX_HOME", "VIBE_HOME", "XDG_DATA_HOME", "CLAUDE_CONFIG_DIR", "OPENCODE_CONFIG")


class TildeTests(ArtifactCase):
    def setUp(self):
        super().setUp()
        self.register(item("x1", "a.txt"), item("x2", "b.txt"))
        self.move_to(self.file_at("plain", "b.txt"), "x2")

    def test_every_variable_reads_a_value_as_the_reference_does(self):
        for variable in VARIABLES:
            for value, accepted in ACCEPTED.items():
                value = value.replace("ME", ME)
                with self.subTest(variable=variable, value=value):
                    state = self.states(env={variable: value})["x2"]
                    if value == "~root/x" and not os.access("/var/root", os.X_OK):
                        # Accepted by `expanduser`, then fails closed: the reference's `_chain`
                        # raises on the EACCES of looking inside root's home.
                        accepted = False
                    self.assertEqual(state, "available" if accepted else "unavailable")

    def test_a_refused_value_ends_only_the_artifact_not_the_command(self):
        done = self.read("task-artifacts", A, "--json", env={"CODEX_HOME": "~nosuchuser/x"})
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_a_default_is_not_expanded_and_an_unset_variable_is_not_refused(self):
        self.assertEqual(self.states()["x2"], "available")

    def test_a_namespace_given_as_another_accounts_home_protects_a_target_there(self):
        """`~<current user>/<name>` is the account database's home, not HOME. A regular file in
        that home is the target; it is read for its status only."""
        import pwd  # POSIX only; the module must still import on Windows CI

        home = pwd.getpwnam(ME).pw_dir
        name = next((n for n in sorted(os.listdir(home))
                     if os.path.isfile(os.path.join(home, n)) and not os.path.islink(os.path.join(home, n))
                     and n.startswith(".") and n not in (".claude.json",) and not n.startswith((".claude", ".codex", ".vibe", ".git", ".config", ".harflow"))), None)
        if name is None:
            self.skipTest("no plain dotfile in the account's home")
        self.sql("UPDATE task_facts SET repo_path=? WHERE id=?", (home, A))
        self.move_to(os.path.join(home, name), "x2")
        self.assertEqual(self.states(env={"CODEX_HOME": "~/nowhere-at-all"})["x2"], "available")
        for variable, value in (("CODEX_HOME", f"~{ME}/{name}"), ("VIBE_HOME", f"~{ME}/{name}")):
            with self.subTest(variable=variable):
                self.assertEqual(self.states(env={variable: value})["x2"], "unavailable")
        self.assertEqual(self.states(env={"CODEX_HOME": f"~{ME}/{name}x"})["x2"], "available", "a different name")

    def test_a_tilde_only_expands_at_the_start(self):
        self.assertEqual(self.states(env={"CODEX_HOME": "/~/x"})["x2"], "available")
        self.assertEqual(self.states(env={"CODEX_HOME": " ~/x"})["x2"], "unavailable")

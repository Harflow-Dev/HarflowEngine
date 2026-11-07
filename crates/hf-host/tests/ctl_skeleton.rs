#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Exit codes and the no-effect guarantee of the public `harflow-ctl` skeleton.
//! Wire shapes are judged by the Python oracle under `tests/`, not here.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::SystemTime,
};

const INVALID_ROOT: &str = "Invalid state root: HARFLOW_STATE_ROOT must be a nonempty absolute path without NUL characters";
const PRIVATE_MODES: [&str; 13] = [
    "--fixture-fixed-protocol-helper",
    "--fixture-claude-qualification-parent",
    "--fixture-claude-qualification-owner",
    "--fixture-claude-qualification-writer",
    "--fixture-claude-qualification-bridge",
    "--fixture-exec-bridge-witness-v1",
    "--fixture-exec-image-witness-v1",
    "--internal-parent-fixture-v1",
    "--internal-owner-fixture-v1",
    "--internal-effect-fixture-v1",
    "--internal-effect-saturation-fixture-v1",
    "--internal-storage-writer-fixture-v1",
    "--internal-isolated-owner-fixture-v1",
];

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Scratch {
    base: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let number = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base =
            std::env::temp_dir().join(format!("hf-ctl-skeleton-{}-{number}", std::process::id()));
        fs::create_dir_all(base.join("home")).unwrap();
        fs::create_dir_all(base.join("work")).unwrap();
        // The temporary directory is a symlink on macOS; roots must not be.
        Self {
            base: fs::canonicalize(&base).unwrap(),
        }
    }

    fn state(&self) -> PathBuf {
        self.base.join("state")
    }

    fn run(&self, args: &[&str], root: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_harflow-ctl"));
        command
            .args(args)
            .current_dir(self.base.join("work"))
            .env("HOME", self.base.join("home"))
            .env("USERPROFILE", self.base.join("home"))
            .env_remove("HARFLOW_TASK_ID")
            .env_remove("HARFLOW_DEPTH")
            .stdin(Stdio::null());
        match root {
            Some(root) => command.env("HARFLOW_STATE_ROOT", root),
            None => command.env_remove("HARFLOW_STATE_ROOT"),
        };
        command.output().unwrap()
    }

    fn tree(&self) -> BTreeMap<PathBuf, (bool, u64, Option<SystemTime>)> {
        let mut entries = BTreeMap::new();
        visit(&self.base, &mut entries);
        entries
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn visit(path: &Path, entries: &mut BTreeMap<PathBuf, (bool, u64, Option<SystemTime>)>) {
    let info = fs::symlink_metadata(path).unwrap();
    entries.insert(
        path.to_path_buf(),
        (info.is_dir(), info.len(), info.modified().ok()),
    );
    if info.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            visit(&child.unwrap().path(), entries);
        }
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn version_info_exits_zero_and_changes_nothing() {
    let scratch = Scratch::new();
    let state = scratch.state();
    let before = scratch.tree();
    for args in [&["version-info"][..], &["version-info", "--json"][..]] {
        let output = scratch.run(args, Some(&state));
        assert_eq!(output.status.code(), Some(0));
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    assert_eq!(scratch.tree(), before);
    assert!(!state.exists());
}

#[test]
fn state_inspect_of_a_missing_root_exits_zero_and_creates_nothing() {
    let scratch = Scratch::new();
    let state = scratch.state();
    let before = scratch.tree();
    let output = scratch.run(&["state-inspect", "--json"], Some(&state));
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("\"status\":\"missing\""));
    assert_eq!(scratch.tree(), before);
    assert!(fs::symlink_metadata(&state).is_err());
}

#[test]
fn state_inspect_of_the_default_root_creates_nothing_under_home() {
    let scratch = Scratch::new();
    let before = scratch.tree();
    let output = scratch.run(&["state-inspect", "--json"], None);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("\"status\":\"missing\""));
    assert_eq!(scratch.tree(), before);
}

#[test]
fn every_inspect_status_exits_zero_and_changes_nothing() {
    let operation = "0123456789abcdef0123456789abcdef";
    for status in ["ready", "incompatible", "interrupted"] {
        let scratch = Scratch::new();
        let state = scratch.state();
        fs::create_dir(&state).unwrap();
        let root = state.to_str().unwrap().replace('\\', "\\\\");
        match status {
            "incompatible" => fs::write(state.join("stray.txt"), "x").unwrap(),
            "interrupted" => {
                let journal = format!(
                    concat!(
                        "{{\"backup_path\":\"{root}.harflow-backup-{op}\",\"format\":\"harflow.state-recovery\",",
                        "\"inventory\":{{}},\"operation_id\":\"{op}\",\"phase\":\"planned\",",
                        "\"product\":\"harflow\",\"proposal\":{{\"operation_id\":\"{op}\",",
                        "\"root_identity\":[1,2],\"state_root\":\"{root}\"}},\"root_identity\":[1,2],",
                        "\"state_root\":\"{root}\",\"v\":1}}"
                    ),
                    root = root,
                    op = operation
                );
                fs::write(state.join(".harflow-recovery.json"), journal).unwrap();
            }
            _ => {}
        }
        let before = scratch.tree();
        for args in [&["state-inspect", "--json"][..], &["state-inspect"][..]] {
            let output = scratch.run(args, Some(&state));
            assert_eq!(output.status.code(), Some(0), "{status}");
            assert!(output.stderr.is_empty(), "{status}");
            assert!(
                stdout(&output).contains(&format!("\"status\": \"{status}\""))
                    || stdout(&output).contains(&format!("\"status\":\"{status}\""))
            );
        }
        assert_eq!(scratch.tree(), before, "{status}");
    }
}

#[test]
fn unimplemented_and_unknown_paths_exit_two_with_a_choice_error() {
    let scratch = Scratch::new();
    let state = scratch.state();
    let before = scratch.tree();
    // The store-backed reads exist on macOS arm64 only; elsewhere they are an invalid choice too.
    let mut commands = vec!["state-recover", "bogus"];
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        commands.extend([
            "list",
            "status",
            "task-events",
            "task-prompt",
            "task-artifacts",
        ]);
    }
    for command in commands {
        let output = scratch.run(&[command], Some(&state));
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert!(output.stdout.is_empty(), "{command}");
        assert!(stderr(&output).contains("invalid choice"), "{command}");
    }
    let missing = scratch.run(&[], Some(&state));
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("the following arguments are required: command"));
    let extra = scratch.run(&["state-inspect", "extra"], Some(&state));
    assert_eq!(extra.status.code(), Some(2));
    assert!(stderr(&extra).contains("unrecognized arguments"));
    assert_eq!(scratch.tree(), before);
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn the_history_commands_print_their_usage_and_status_has_brief() {
    let scratch = Scratch::new();
    for (command, option) in [
        ("task-events", "--all-kinds"),
        ("task-prompt", "--max-bytes"),
        ("task-artifacts", "--cursor"),
        ("status", "--brief"),
    ] {
        let output = scratch.run(&[command, "-h"], None);
        assert_eq!(output.status.code(), Some(0), "{command}");
        let usage = stdout(&output);
        assert!(
            usage.starts_with(&format!("usage: harflow-ctl {command}")),
            "{usage}"
        );
        assert!(usage.contains(option), "{command}: {usage}");
    }
}

#[test]
fn an_invalid_root_exits_two_before_the_command_is_parsed() {
    let scratch = Scratch::new();
    let before = scratch.tree();
    for root in ["relative/state", "."] {
        for args in [
            &["bogus"][..],
            &["state-inspect"][..],
            &["version-info"][..],
        ] {
            let output = scratch.run(args, Some(Path::new(root)));
            assert_eq!(output.status.code(), Some(2), "{root} {args:?}");
            assert!(stderr(&output).contains(INVALID_ROOT), "{root} {args:?}");
            assert!(!stderr(&output).contains("invalid choice"));
        }
    }
    assert_eq!(scratch.tree(), before);
}

#[test]
fn help_exits_zero_even_with_an_invalid_root() {
    let scratch = Scratch::new();
    let output = scratch.run(&["-h"], Some(Path::new("relative")));
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).starts_with("usage: harflow-ctl"));
}

#[test]
fn private_fixture_modes_are_refused_without_effect() {
    let scratch = Scratch::new();
    let state = scratch.state();
    let baseline = scratch.run(&["version-info"], Some(&state));
    assert_eq!(baseline.status.code(), Some(0));
    let before = scratch.tree();
    for mode in PRIVATE_MODES {
        for extra in [&[][..], &["extra"][..]] {
            let mut args = vec![mode];
            args.extend_from_slice(extra);
            let output = scratch.run(&args, Some(&state));
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(output.stdout.is_empty(), "{args:?}");
            assert!(
                stderr(&output).starts_with("harflow-ctl: error: "),
                "{args:?}"
            );
        }
    }
    assert_eq!(scratch.tree(), before);
    assert!(!state.exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_existing_root_is_reported_without_following_it() {
    let scratch = Scratch::new();
    let target = scratch.base.join("real");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("stray.txt"), "x").unwrap();
    let link = scratch.state();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let before = scratch.tree();
    let output = scratch.run(&["state-inspect", "--json"], Some(&link));
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("\"reason\":\"unsafe_root\""));
    assert_eq!(scratch.tree(), before);
}

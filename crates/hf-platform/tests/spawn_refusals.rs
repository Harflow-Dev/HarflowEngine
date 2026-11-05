#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::process::{MacIdentity, RetainedOwnedChild, SpawnSpec};
use std::{
    ffi::OsString,
    fs,
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-spawn-refusal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn effect_path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

fn spec(sentinel: &Path) -> SpawnSpec {
    SpawnSpec {
        executable: PathBuf::from("/usr/bin/touch"),
        cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        args: vec![sentinel.as_os_str().to_owned()],
        environment: Vec::new(),
        capture_limit: 0,
    }
}

fn assert_refused(spec: SpawnSpec, until: Instant) {
    match RetainedOwnedChild::spawn(spec, &MacIdentity, until) {
        Err(_) => (),
        Ok(mut child) => {
            child.close_stdin();
            let _ = child.wait_until(Instant::now() + Duration::from_secs(2), || false);
            panic!("invalid spawn specification was admitted");
        }
    }
}

#[test]
fn noninherited_working_directory_and_invalid_executable_paths_refuse_before_effects() {
    let scratch = Scratch::new();
    let sentinel = scratch.effect_path("spawned");
    let mut different_cwd = spec(&sentinel);
    different_cwd.cwd = fs::canonicalize(std::env::temp_dir()).unwrap();
    assert_ne!(
        different_cwd.cwd,
        fs::canonicalize(std::env::current_dir().unwrap()).unwrap()
    );
    assert_refused(different_cwd, deadline());

    let mut relative = spec(&sentinel);
    relative.executable = PathBuf::from("touch");
    assert_refused(relative, deadline());

    let symlink = scratch.effect_path("touch-link");
    std::os::unix::fs::symlink("/usr/bin/touch", &symlink).unwrap();
    let mut linked = spec(&sentinel);
    linked.executable = symlink;
    assert_refused(linked, deadline());

    let nonexecutable = scratch.effect_path("not-executable");
    fs::write(&nonexecutable, b"not an executable").unwrap();
    fs::set_permissions(&nonexecutable, fs::Permissions::from_mode(0o600)).unwrap();
    let mut denied = spec(&sentinel);
    denied.executable = nonexecutable;
    assert_refused(denied, deadline());
    assert!(
        !sentinel.exists(),
        "a rejected request ran its effect helper"
    );
}

#[test]
fn malformed_environment_keys_values_and_arguments_refuse_before_effects() {
    let scratch = Scratch::new();
    let sentinel = scratch.effect_path("spawned");
    let invalid_environments = [
        vec![(OsString::new(), OsString::from("value"))],
        vec![(OsString::from("A=B"), OsString::from("value"))],
        vec![
            (OsString::from("DUP"), OsString::from("one")),
            (OsString::from("DUP"), OsString::from("two")),
        ],
        vec![(
            OsString::from_vec(b"BAD\0KEY".to_vec()),
            OsString::from("v"),
        )],
        vec![(
            OsString::from("KEY"),
            OsString::from_vec(b"BAD\0VALUE".to_vec()),
        )],
    ];
    for environment in invalid_environments {
        let mut invalid = spec(&sentinel);
        invalid.environment = environment;
        assert_refused(invalid, deadline());
        assert!(
            !sentinel.exists(),
            "invalid environment launched its helper"
        );
    }

    let mut invalid_arg = spec(&sentinel);
    invalid_arg
        .args
        .push(OsString::from_vec(b"BAD\0ARG".to_vec()));
    assert_refused(invalid_arg, deadline());
    assert!(!sentinel.exists(), "NUL argument launched its helper");
}

#[test]
fn expired_spawn_deadline_refuses_without_running_the_effect_helper() {
    let scratch = Scratch::new();
    let sentinel = scratch.effect_path("spawned");
    assert_refused(spec(&sentinel), Instant::now());
    assert!(!sentinel.exists(), "expired request ran its effect helper");
}

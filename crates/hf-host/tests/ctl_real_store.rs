#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `harflow-ctl state-inspect` on a root built by the real storage initializer.
use hf_storage::{Limits, Store};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tree(path: &Path, out: &mut BTreeMap<PathBuf, (u64, Option<SystemTime>)>) {
    let info = fs::symlink_metadata(path).unwrap();
    out.insert(path.to_path_buf(), (info.len(), info.modified().ok()));
    if info.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            tree(&child.unwrap().path(), out);
        }
    }
}

#[test]
fn a_root_from_the_real_initializer_inspects_as_ready_without_changes() {
    let base = std::env::temp_dir().join(format!("hf-ctl-real-store-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    let scratch = Scratch(fs::canonicalize(&base).unwrap());
    let root = scratch.0.join("store");
    drop(
        Store::initialize_fixture(
            &root,
            Limits::default(),
            Instant::now() + Duration::from_secs(10),
            hf_platform::lease::shared(),
        )
        .unwrap(),
    );
    let mut before = BTreeMap::new();
    tree(&scratch.0, &mut before);
    let output = Command::new(env!("CARGO_BIN_EXE_harflow-ctl"))
        .arg("state-inspect")
        .arg("--json")
        .env("HARFLOW_STATE_ROOT", &root)
        .env_remove("HARFLOW_TASK_ID")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\"status\": \"ready\""), "{text}");
    assert!(text.contains("\"reason\": \"compatible\""), "{text}");
    let mut after = BTreeMap::new();
    tree(&scratch.0, &mut after);
    assert_eq!(before, after);
}

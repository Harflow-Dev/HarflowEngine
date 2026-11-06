#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic)]

//! Preparation-only checks. The helper-spawning cases are deliberately not
//! included here until the Q0 manifest and OS measurement envelope are approved.
use hf_platform::st04_fixture::absence_profile;
#[path = "st04_helpers/support/mod.rs"]
mod support;
use hf_platform::st04_fixture::ProbeControl;
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = Path::new("/private/tmp").join(format!(
            "hf-st04-profile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create profile-only fixture");
        Self(fs::canonicalize(path).expect("canonical profile-only fixture"))
    }
    fn helper(&self) -> PathBuf {
        let path = self.0.join("fixed-helper");
        fs::write(&path, b"reviewed helper placeholder").expect("write helper placeholder");
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove process-free profile fixture");
    }
}

#[test]
fn absence_profile_is_deny_by_default_and_has_only_exact_helper_exec() {
    let scratch = Scratch::new();
    let helper = scratch.helper();
    let profile = absence_profile(&helper).expect("render fixed absence profile");

    assert!(profile.contains("(deny default)"));
    assert!(profile.contains("(allow process-exec (literal"));
    assert!(
        profile.contains(
            "(deny file-write* process-fork network* signal mach-lookup appleevent-send)"
        )
    );
    assert!(profile.contains("(subpath \"/System/Library\")"));
    assert!(profile.contains("(subpath \"/usr/lib\")"));
    assert!(profile.contains(&helper.to_string_lossy().to_string()));
    assert!(!profile.contains("(allow network"));
    assert!(!profile.contains("(allow file-write"));
}

#[test]
fn absence_profile_refuses_symlink_and_relative_helper_paths() {
    let scratch = Scratch::new();
    let helper = scratch.helper();
    let alias = scratch.0.join("helper-alias");
    symlink(&helper, &alias).expect("create alias fixture");

    assert!(
        absence_profile(&alias).is_err(),
        "logical helper aliases refuse"
    );
    assert!(
        absence_profile(Path::new("relative-helper")).is_err(),
        "relative helper paths refuse"
    );
}

#[test]
fn absence_profile_refuses_directories_and_oversized_helpers() {
    let scratch = Scratch::new();
    let directory = scratch.0.join("directory");
    fs::create_dir(&directory).expect("create directory fixture");
    assert!(absence_profile(&directory).is_err());

    let oversized = scratch.0.join("oversized-helper");
    fs::write(&oversized, vec![b'x'; 1_048_577]).expect("write oversized fixture");
    assert!(absence_profile(&oversized).is_err());
}

#[test]
#[ignore = "requires Q0 manifest/profile review and explicit OS helper qualification authorization"]
fn actual_fixed_helper_absence_and_single_permission_controls() {
    for control in [
        ProbeControl::None,
        ProbeControl::Write,
        ProbeControl::Read,
        ProbeControl::Fork,
        ProbeControl::Network,
        ProbeControl::Signal,
    ] {
        support::run_case(control);
    }
}

//! Own process: the root is created under a restrictive umask by a re-executed child.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::Store;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const CASE: &str = "new_root_is_owner_only_under_umask_022";

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn new_root_is_owner_only_under_umask_022() {
    if std::env::var_os("HF_ROOT_MODE_CHILD").is_none() {
        let status = Command::new("/bin/sh")
            .args([
                "-c",
                "umask 022; exec \"$0\" --exact \"$1\" --test-threads=1",
            ])
            .arg(std::env::current_exe().unwrap())
            .arg(CASE)
            .env("HF_ROOT_MODE_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success(), "the child run under umask 022 failed");
        return;
    }
    let fixture = Fixture::new();
    // A plain directory creation under this umask is group- and other-readable.
    let control = fixture.path().join("control");
    std::fs::create_dir(&control).unwrap();
    assert_eq!(mode(&control), 0o755);
    let root = fixture.path().join("root");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root) & 0o077, 0);
}

#[test]
fn an_existing_root_is_not_changed() {
    let fixture = Fixture::new();
    let root = fixture.path().join("existing");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    let refused = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    );
    assert!(refused.is_err());
    assert_eq!(mode(&root), 0o755);
}

#![cfg(target_os = "macos")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The macOS recovery filesystem adapter: lock creation and the free-space reading.
use hf_application::ctl::state_recover::RecoveryFsPort;
use hf_platform::ctl::recovery_fs::MacRecoveryFs;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-recovery-fs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn create_lock_creates_an_empty_file_with_mode_0600() {
    let fixture = Fixture::new();
    assert!(MacRecoveryFs.create_lock(&fixture.0, "store.lock").unwrap());
    let metadata = fs::symlink_metadata(fixture.0.join("store.lock")).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.len(), 0);
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[test]
fn create_lock_over_an_existing_file_is_not_created_and_not_an_error() {
    let fixture = Fixture::new();
    let path = fixture.0.join("io.lock");
    fs::write(&path, b"kept").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    assert!(!MacRecoveryFs.create_lock(&fixture.0, "io.lock").unwrap());
    // The existing file is untouched.
    assert_eq!(fs::read(&path).unwrap(), b"kept");
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn create_lock_refuses_to_follow_a_symlink() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target");
    fs::write(&target, b"outside").unwrap();
    // A link to an existing file is an existing entry: not created, target untouched.
    std::os::unix::fs::symlink(&target, fixture.0.join("store.lock")).unwrap();
    assert!(!MacRecoveryFs.create_lock(&fixture.0, "store.lock").unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"outside");
    // A dangling link is also an existing entry: nothing is created through it.
    let missing = fixture.0.join("missing");
    std::os::unix::fs::symlink(&missing, fixture.0.join("io.lock")).unwrap();
    assert!(!MacRecoveryFs.create_lock(&fixture.0, "io.lock").unwrap());
    assert!(!missing.exists());
    assert!(
        fs::symlink_metadata(fixture.0.join("io.lock"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn create_lock_refuses_names_that_leave_the_root_and_a_missing_root() {
    let fixture = Fixture::new();
    for name in ["", ".", "..", "a/b", "../escape"] {
        let error = MacRecoveryFs.create_lock(&fixture.0, name).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{name:?}");
    }
    assert!(!fixture.0.join("escape").exists());
    let missing = fixture.0.join("no-such-root");
    let error = MacRecoveryFs
        .create_lock(&missing, "store.lock")
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn free_space_reports_the_volume_and_refuses_a_missing_path() {
    let fixture = Fixture::new();
    let space = MacRecoveryFs.free_space(&fixture.0).unwrap();
    assert!(space > 0);
    // The same volume reports a stable order of magnitude for a file inside it.
    let file = fixture.0.join("file");
    fs::write(&file, b"x").unwrap();
    let again = MacRecoveryFs.free_space(&file).unwrap();
    assert!(again > 0);
    assert!(
        MacRecoveryFs
            .free_space(&fixture.0.join("missing"))
            .is_err()
    );
}

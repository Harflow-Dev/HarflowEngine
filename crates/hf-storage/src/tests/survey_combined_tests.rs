#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Combined faults through `precheck` and the survey's per-check record. A store with
//! several faults reports the first one in the order the pre-survey `precheck` made its
//! checks: sidecars, marker, header, rollback journal, `-wal`, acceptance record, fence,
//! watermark, binding.
use crate::{
    Error, Limits, Store,
    binding::Binding,
    maintenance::{MaintenanceSession, SessionRequest},
    qualify::precheck,
    survey::{Check, Live, Mode, Outcome, run},
};
use hf_application::ctl::state_recover::RecoveryFsPort;
use hf_domain::capture::FileIdentity;
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestFs;
impl RecoveryFsPort for TestFs {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join(name))
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}

fn store(name: &str) -> (Scratch, PathBuf, String) {
    let parent =
        std::env::temp_dir().join(format!("hf-storage-combined-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = crate::tests::lease::lease_port();
    let deadline = Instant::now() + Duration::from_secs(5);
    let opened = Store::initialize_fixture(&root, Limits::default(), deadline, leases).unwrap();
    let generation = opened.identity().generation.clone();
    drop(opened);
    (scratch, root, generation)
}

fn binding(root: &Path) -> Binding {
    Binding::identify_existing(root, crate::tests::lease::lease_port()).unwrap()
}

fn id(path: &Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn session(root: &Path) -> MaintenanceSession {
    let request = SessionRequest {
        root_identity: id(root),
        lock_identity: Some(id(&root.join("store.lock"))),
    };
    let leases = crate::tests::lease::lease_port();
    MaintenanceSession::acquire(root, &request, Arc::new(TestFs), leases).unwrap()
}

fn fence(root: &Path, generation: &str, state: &str) {
    let mut file = fs::File::create(root.join("store.fence")).unwrap();
    write!(file, "harflow-fence-1\n{generation}\n{state}\n").unwrap();
}

fn remove_wal(root: &Path) {
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
}

/// A symbolic link that names itself: opening it follows the link and fails with `ELOOP`.
fn looping_fence(root: &Path) {
    std::os::unix::fs::symlink("store.fence", root.join("store.fence")).unwrap();
}

/// A directory where the watermark file belongs: it cannot be read as a file.
fn unreadable_watermark(root: &Path) {
    let path = root.join("store.watermark");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
}

#[test]
fn missing_wal_and_looping_fence_report_the_missing_wal() {
    let (_scratch, root, _) = store("wal-eloop");
    // Alone, the fence is the failure.
    looping_fence(&root);
    assert!(matches!(
        precheck(&binding(&root)),
        Err(Error::Incompatible | Error::Io(_))
    ));
    remove_wal(&root);
    assert!(matches!(precheck(&binding(&root)), Err(Error::WalMissing)));
}

#[test]
fn missing_wal_and_unreadable_watermark_report_the_missing_wal() {
    let (_scratch, root, _) = store("wal-watermark");
    unreadable_watermark(&root);
    assert!(matches!(
        precheck(&binding(&root)),
        Err(error) if !matches!(error, Error::WalMissing)
    ));
    remove_wal(&root);
    assert!(matches!(precheck(&binding(&root)), Err(Error::WalMissing)));
}

#[test]
fn nonempty_journal_and_missing_wal_report_the_journal() {
    let (_scratch, root, _) = store("journal-wal");
    fs::write(root.join("store.sqlite3-journal"), b"pending").unwrap();
    assert!(matches!(
        precheck(&binding(&root)),
        Err(Error::NeedsCheckpoint)
    ));
    remove_wal(&root);
    assert!(matches!(
        precheck(&binding(&root)),
        Err(Error::NeedsCheckpoint)
    ));
}

#[test]
fn bad_header_and_fence_report_the_header() {
    let (_scratch, root, generation) = store("header-fence");
    fence(&root, &generation, "commit_lost");
    assert!(matches!(precheck(&binding(&root)), Err(Error::CommitLost)));
    let mut bytes = fs::read(root.join("store.sqlite3")).unwrap();
    bytes[0] = b'X';
    fs::write(root.join("store.sqlite3"), bytes).unwrap();
    assert!(matches!(
        precheck(&binding(&root)),
        Err(Error::Incompatible)
    ));
}

#[test]
fn replaced_binding_beats_a_clean_or_later_failing_check() {
    let (_scratch, root, generation) = store("replaced");
    let bound = binding(&root);
    // Every file check passes on the new file; only the binding differs.
    fs::write(root.join("store.lock.new"), b"").unwrap();
    fs::rename(root.join("store.lock.new"), root.join("store.lock")).unwrap();
    assert!(matches!(
        crate::survey::precheck(&bound),
        Err(Error::Replaced)
    ));
    // A later fault does not change which error comes first.
    fence(&root, &generation, "commit_lost");
    assert!(matches!(
        crate::survey::precheck(&bound),
        Err(Error::Replaced)
    ));
    // A fresh binding of the replaced root sees only the fence.
    assert!(matches!(precheck(&binding(&root)), Err(Error::CommitLost)));
}

#[test]
fn survey_records_each_check() {
    let (_scratch, root, generation) = store("records");
    let held = session(&root);
    let clean = held.survey().unwrap();
    for check in [
        Check::Sidecars,
        Check::Marker,
        Check::Header,
        Check::Journal,
        Check::Wal,
        Check::Acceptance,
        Check::Fence,
        Check::Watermark,
        Check::Binding,
    ] {
        assert!(
            matches!(clean.outcome(check), Outcome::Passed),
            "{check:?} on a clean store"
        );
    }
    assert_eq!(clean.identity().unwrap().generation, generation);
    assert!(clean.mark().is_some());
    assert!(clean.require_clean().is_ok());

    // Four independent faults are recorded together, each against its own check.
    fs::write(root.join("store.sqlite3-journal"), b"pending").unwrap();
    remove_wal(&root);
    fence(&root, &generation, "commit_lost");
    unreadable_watermark(&root);
    let survey = held.survey().unwrap();
    assert!(matches!(
        survey.outcome(Check::Journal),
        Outcome::Failed(Error::NeedsCheckpoint)
    ));
    assert!(matches!(
        survey.outcome(Check::Wal),
        Outcome::Failed(Error::WalMissing)
    ));
    assert!(matches!(
        survey.outcome(Check::Fence),
        Outcome::Failed(Error::CommitLost)
    ));
    assert!(matches!(
        survey.outcome(Check::Watermark),
        Outcome::Failed(_)
    ));
    assert!(survey.mark().is_none());
    assert!(matches!(survey.outcome(Check::Binding), Outcome::Passed));
    // Replayed in precheck order, the first failure is the journal.
    assert!(matches!(
        survey.require_clean(),
        Err(Error::NeedsCheckpoint)
    ));
}

/// An unclean survey never yields a clean result in either mode. This is the behavioural
/// half only: that no code outside the session can obtain a maintenance survey is a matter
/// of visibility (`Authority` has a private constructor in the `maintenance` module and
/// `MaintenanceSession::survey` is `pub(crate)`), which a runtime test cannot show.
#[test]
fn unclean_survey_never_yields_a_clean_result() {
    let (_scratch, root, generation) = store("unusable");
    let held = session(&root);
    remove_wal(&root);
    fence(&root, &generation, "commit_lost");
    let maintenance = held.survey().unwrap();
    assert!(maintenance.require_clean().is_err());
    let precheck = run(&binding(&root), Mode::Precheck, &Live);
    // The precheck mode stops at the first failure and records the rest as not performed.
    assert!(matches!(
        precheck.outcome(Check::Wal),
        Outcome::Failed(Error::WalMissing)
    ));
    assert!(matches!(
        precheck.outcome(Check::Fence),
        Outcome::NotPerformed
    ));
    assert!(precheck.identity().is_some());
    assert!(precheck.mark().is_none());
    assert!(precheck.require_clean().is_err());
}

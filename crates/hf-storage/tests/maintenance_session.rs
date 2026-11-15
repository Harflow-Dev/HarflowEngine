#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The maintenance session: lock creation and exclusion, identity checks, and the owner
//! evidence read.
mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::ctl::state_recover::{RecoveryFsPort, evidence::Basis};
use hf_domain::capture::FileIdentity;
use hf_storage::{Error, EvidenceBounds, MaintenanceSession, Mutation, SessionRequest, Store};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const OP: &str = "0123456789abcdef0123456789abcdef";

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

fn store(scratch: &Fixture) -> PathBuf {
    let root = scratch.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    store
        .apply_batch(
            &[Mutation::Put {
                key: "k".into(),
                value: b"v".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    root
}

fn id(path: &Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn acquire(root: &Path, lock: Option<FileIdentity>) -> hf_storage::Result<MaintenanceSession> {
    let request = SessionRequest {
        root_identity: id(root),
        lock_identity: lock,
    };
    MaintenanceSession::acquire_within(
        root,
        &request,
        Arc::new(TestFs),
        common::lease::lease_port(),
        Duration::from_millis(100),
    )
}

#[test]
fn absent_locks_are_created_and_a_null_token_identity_stays_null() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.lock")).unwrap();
    fs::remove_file(root.join("io.lock")).unwrap();
    let session = acquire(&root, None).unwrap();
    assert!(session.store_lock_created() && session.io_lock_created());
    assert_eq!(session.lock_identity(), &id(&root.join("store.lock")));
    assert_eq!(session.comparable_lock_identity(None), None);
    drop(session);
    // A retry before any journal: the lock now exists and the null token still matches.
    let retry = acquire(&root, None).unwrap();
    assert!(!retry.store_lock_created());
    assert_eq!(retry.comparable_lock_identity(None), None);
    let named = retry.lock_identity().clone();
    assert_eq!(retry.comparable_lock_identity(Some(&named)), Some(named));
}

#[test]
fn a_lock_identity_that_is_not_the_listed_one_is_stale() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let wrong = FileIdentity {
        device: 1,
        inode: 1,
    };
    assert!(matches!(acquire(&root, Some(wrong)), Err(Error::Replaced)));
    let mut request = SessionRequest {
        root_identity: id(&root),
        lock_identity: None,
    };
    request.root_identity.inode += 1;
    let refused = MaintenanceSession::acquire(
        &root,
        &request,
        Arc::new(TestFs),
        common::lease::lease_port(),
    );
    assert!(matches!(refused, Err(Error::Replaced)));
}

#[test]
fn a_live_store_handle_exhausts_the_budget_as_busy() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let held = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert!(matches!(acquire(&root, None), Err(Error::Busy)));
    drop(held);
    assert!(acquire(&root, None).is_ok());
}

#[test]
fn a_healthy_store_has_clear_evidence_read_in_place() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let session = acquire(&root, None).unwrap();
    let set = session.evidence(OP, EvidenceBounds::default()).unwrap();
    assert_eq!(set.basis, Basis::Clear);
    assert!(set.items.is_empty());
}

#[test]
fn a_missing_wal_reads_a_copy_and_never_creates_wal_or_leaves_scratch() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let session = acquire(&root, None).unwrap();
    let set = session.evidence(OP, EvidenceBounds::default()).unwrap();
    assert_eq!(set.basis, Basis::EarlierSnapshot);
    assert!(!root.join("store.sqlite3-wal").exists());
    let leftovers: Vec<_> = fs::read_dir(scratch.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, vec![std::ffi::OsString::from("store")]);
}

#[test]
fn an_unreadable_header_is_unreadable_evidence_and_a_failed_copy_is_too() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let session = acquire(&root, None).unwrap();
    // A past copy deadline fails the copy; the source stays untouched.
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let bounds = EvidenceBounds {
        copy: Duration::ZERO,
        queries: Duration::from_secs(5),
    };
    let set = session.evidence(OP, bounds).unwrap();
    assert_eq!(set.basis, Basis::Unreadable);
    assert!(!root.join("store.sqlite3-wal").exists());
    drop(session);
    let mut bytes = fs::read(root.join("store.sqlite3")).unwrap();
    bytes[0] = b'X';
    fs::write(root.join("store.sqlite3"), bytes).unwrap();
    let session = acquire(&root, None).unwrap();
    let set = session.evidence(OP, EvidenceBounds::default()).unwrap();
    assert_eq!(set.basis, Basis::Unreadable);
}

#[test]
fn recorded_task_processes_are_read_whether_terminal_or_not() {
    use hf_application::ctl::state_recover::evidence::{Source, Subject};
    let scratch = Fixture::new();
    let root = store(&scratch);
    let db = common::raw_connection(root.join("store.sqlite3"));
    db.execute(
        "INSERT INTO task_processes(task,pid,start_seconds,start_micros,boot_uuid) \
         VALUES('a',4242,1700000000,5,'abcdef01-2345-6789-abcd-ef0123456789')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO task_processes(task,pid) VALUES('b',4343)", [])
        .unwrap();
    drop(db);
    let session = acquire(&root, None).unwrap();
    let set = session.evidence(OP, EvidenceBounds::default()).unwrap();
    assert_eq!(set.basis, Basis::Clear);
    assert_eq!(set.items.len(), 2);
    assert!(
        set.items
            .iter()
            .all(|item| item.source == Source::TaskProcess)
    );
    assert!(
        set.items
            .iter()
            .any(|item| matches!(item.subject, Subject::Pid(4343)))
    );
}

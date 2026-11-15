#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Lock identity handling of the maintenance session: a token that names no lock, a retry
//! after the lock was created, and a lock file that is not the one acquired.
mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::{
    ctl::state_recover::RecoveryFsPort,
    lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort},
};
use hf_domain::capture::FileIdentity;
use hf_storage::{Error, MaintenanceSession, SessionRequest, Store};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

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

/// Delegates to the file lease; once, right after `target` is acquired, replaces that file
/// by another regular file (a new inode) as a racing actor would.
struct SwappingLease {
    inner: Arc<dyn LeasePort>,
    target: PathBuf,
    swapped: AtomicBool,
}
impl LeasePort for SwappingLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        let guard = self.inner.acquire(path, mode, create, deadline)?;
        if path == self.target && !self.swapped.swap(true, Ordering::SeqCst) {
            let staged = path.with_extension("swap");
            fs::write(&staged, b"").unwrap();
            fs::rename(&staged, path).unwrap();
        }
        Ok(guard)
    }
}

fn store(scratch: &Fixture) -> PathBuf {
    let root = scratch.path().join("store");
    Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
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

fn request(root: &Path, lock: Option<FileIdentity>) -> SessionRequest {
    SessionRequest {
        root_identity: id(root),
        lock_identity: lock,
    }
}

fn acquire_with(
    root: &Path,
    request: &SessionRequest,
    leases: Arc<dyn LeasePort>,
) -> hf_storage::Result<MaintenanceSession> {
    MaintenanceSession::acquire_within(
        root,
        request,
        Arc::new(TestFs),
        leases,
        Duration::from_millis(200),
    )
}

#[test]
fn first_recovery_from_lockless_root_matches_token() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.lock")).unwrap();
    // The inspection saw no lock, so the token's identity is null.
    let token = request(&root, None);
    let session = acquire_with(&root, &token, common::lease::lease_port()).unwrap();
    assert!(session.store_lock_created());
    // The journal records the inode that was acquired, which now exists ...
    let created = id(&root.join("store.lock"));
    assert_eq!(session.lock_identity(), &created);
    // ... while the comparison with the token stays null, so the first recovery matches.
    assert_eq!(
        session.comparable_lock_identity(token.lock_identity.as_ref()),
        None
    );
}

#[test]
fn pre_journal_retry_after_lock_created_matches_token() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.lock")).unwrap();
    let token = request(&root, None);
    let first = acquire_with(&root, &token, common::lease::lease_port()).unwrap();
    let created = first.lock_identity().clone();
    // The first attempt stops before the journal exists.
    drop(first);
    let retry = acquire_with(&root, &token, common::lease::lease_port()).unwrap();
    assert!(!retry.store_lock_created());
    assert_eq!(retry.lock_identity(), &created);
    assert_eq!(
        retry.comparable_lock_identity(token.lock_identity.as_ref()),
        None
    );
    // A token that names a lock other than the one now listed is still stale.
    drop(retry);
    let wrong = FileIdentity {
        device: created.device,
        inode: created.inode + 1,
    };
    let stale = acquire_with(
        &root,
        &request(&root, Some(wrong)),
        common::lease::lease_port(),
    );
    assert!(matches!(stale, Err(Error::Replaced)));
}

#[test]
fn acquired_lock_inode_differs_from_listed_is_stale() {
    for name in ["store.lock", "io.lock"] {
        let scratch = Fixture::new();
        let root = store(&scratch);
        let token = request(&root, Some(id(&root.join("store.lock"))));
        let leases = Arc::new(SwappingLease {
            inner: common::lease::lease_port(),
            target: root.join(name),
            swapped: AtomicBool::new(false),
        });
        let refused = acquire_with(&root, &token, leases);
        assert!(matches!(refused, Err(Error::Replaced)), "{name}");
    }
    // Without the swap the same request succeeds.
    let scratch = Fixture::new();
    let root = store(&scratch);
    let token = request(&root, Some(id(&root.join("store.lock"))));
    assert!(acquire_with(&root, &token, common::lease::lease_port()).is_ok());
}

/// Creates `io.lock` as the plain port does, but first replaces `store.lock` by another
/// regular file: a swap between the first listing and the second.
struct SwapBetweenListings(PathBuf);
impl RecoveryFsPort for SwapBetweenListings {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        let lock = self.0.join("store.lock");
        let staged = self.0.join("store.lock.swap");
        fs::write(&staged, b"")?;
        fs::rename(&staged, lock)?;
        TestFs.create_lock(root, name)
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}

#[test]
fn a_requested_lock_swapped_between_the_listings_is_stale() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("io.lock")).unwrap();
    let token = request(&root, Some(id(&root.join("store.lock"))));
    let refused = MaintenanceSession::acquire_within(
        &root,
        &token,
        Arc::new(SwapBetweenListings(root.clone())),
        common::lease::lease_port(),
        Duration::from_millis(200),
    );
    assert!(matches!(refused, Err(Error::Replaced)));
}

#[test]
fn a_root_replaced_after_acquisition_is_replaced_and_nothing_is_read_from_it() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let token = request(&root, Some(id(&root.join("store.lock"))));
    let session = acquire_with(&root, &token, common::lease::lease_port()).unwrap();
    fs::rename(&root, scratch.path().join("moved")).unwrap();
    let replacement = store(&scratch);
    assert_eq!(replacement, root);
    let op = "0123456789abcdef0123456789abcdef";
    let refused = session.evidence(op, hf_storage::EvidenceBounds::default());
    assert!(matches!(refused, Err(Error::Replaced)));
}

/// Reports plenty of space, but moves the root away and puts another directory in its
/// place the moment the evidence read asks, in the middle of the copy.
struct SwapDuringRead(PathBuf);
impl RecoveryFsPort for SwapDuringRead {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        TestFs.create_lock(root, name)
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        fs::rename(&self.0, self.0.with_file_name("moved"))?;
        fs::create_dir(&self.0)?;
        Ok(u64::MAX)
    }
}

#[test]
fn a_root_swapped_during_the_read_is_replaced() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let token = request(&root, Some(id(&root.join("store.lock"))));
    let session = MaintenanceSession::acquire_within(
        &root,
        &token,
        Arc::new(SwapDuringRead(root.clone())),
        common::lease::lease_port(),
        Duration::from_millis(200),
    )
    .unwrap();
    let op = "0123456789abcdef0123456789abcdef";
    let refused = session.evidence(op, hf_storage::EvidenceBounds::default());
    assert!(matches!(refused, Err(Error::Replaced)));
}

/// Fails the copy before SQLite opens it (no space), after another actor put a regular
/// `store.sqlite3-wal` into the scratch directory.
struct InjectThenRefuse(PathBuf);
impl RecoveryFsPort for InjectThenRefuse {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        TestFs.create_lock(root, name)
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        let op = "0123456789abcdef0123456789abcdef";
        fs::write(
            self.0
                .join(format!(".store.harflow-evidence-{op}"))
                .join("store.sqlite3-wal"),
            b"theirs",
        )?;
        Ok(0)
    }
}

#[test]
fn a_sidecar_added_during_a_copy_that_fails_before_open_is_not_deleted() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let token = request(&root, Some(id(&root.join("store.lock"))));
    let session = MaintenanceSession::acquire_within(
        &root,
        &token,
        Arc::new(InjectThenRefuse(scratch.path().to_path_buf())),
        common::lease::lease_port(),
        Duration::from_millis(200),
    )
    .unwrap();
    let op = "0123456789abcdef0123456789abcdef";
    let outcome = session.evidence(op, hf_storage::EvidenceBounds::default());
    assert!(matches!(outcome, Err(Error::Io(_))));
    let injected = scratch
        .path()
        .join(format!(".store.harflow-evidence-{op}"))
        .join("store.sqlite3-wal");
    assert_eq!(fs::read(injected).unwrap(), b"theirs");
}

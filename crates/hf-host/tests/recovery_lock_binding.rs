#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::ctl::state_inspect::{
    OperationIdPort, ProposalPorts, StorePolicy, inspect_state,
};
use hf_domain::{
    capture::FileIdentity as SessionIdentity,
    ctl::{recovery_proposal::FileIdentity, state_root::StateRoot},
};
use hf_interfaces::ctl::{recovery_codec::JsonRecoveryCodec, state_codec::JsonStateCodec};
use hf_platform::ctl::{
    inventory::FsInventory, recovery_fs::MacRecoveryFs, state_observer::FsStateObserver,
};
use hf_storage::{Error, MaintenanceSession, SessionRequest, Sha256Digest};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hf-recovery-lock-binding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixedIds;
impl OperationIdPort for FixedIds {
    fn fresh(&self) -> Option<String> {
        Some("0123456789abcdef0123456789abcdef".into())
    }
}

fn identity(path: &Path) -> SessionIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    SessionIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn proposal_lock(root: &Path) -> Option<FileIdentity> {
    let root = StateRoot::from_normalized(root.to_string_lossy().into_owned());
    let policy = StorePolicy {
        schema_version: hf_storage::SCHEMA_VERSION,
        application_id: hf_storage::APPLICATION_ID,
    };
    let ports = ProposalPorts {
        inventory: &FsInventory,
        codec: &JsonRecoveryCodec,
        digest: &Sha256Digest,
        ids: &FixedIds,
    };
    let inspection = inspect_state(&FsStateObserver, &JsonStateCodec, policy, &ports, &root);
    inspection
        .recovery
        .expect("an incompatible owned fixture should be proposable")
        .proposal
        .lock_identity
}

fn make_incompatible(root: &Path) {
    fs::write(root.join("store.identity"), b"invalid marker").unwrap();
}

fn acquire(
    root: &Path,
    lock_identity: Option<FileIdentity>,
) -> hf_storage::Result<MaintenanceSession> {
    MaintenanceSession::acquire_within(
        root,
        &SessionRequest {
            root_identity: identity(root),
            lock_identity: lock_identity.map(|id| SessionIdentity {
                device: id.dev as u64,
                inode: id.ino,
            }),
        },
        Arc::new(MacRecoveryFs),
        hf_platform::lease::shared(),
        Duration::from_millis(250),
    )
}

#[test]
fn proposal_binds_store_lock_and_session_admits_the_same_store_lock() {
    let fixture = Fixture::new();
    make_incompatible(&fixture.0);
    for name in ["store.lock", "io.lock", ".harflow-store.lock"] {
        fs::write(fixture.0.join(name), b"distinct bytes").unwrap();
    }
    let expected = proposal_lock(&fixture.0).unwrap();
    assert_eq!(
        expected,
        FileIdentity {
            dev: i64::from(identity(&fixture.0.join("store.lock")).device as i32),
            ino: identity(&fixture.0.join("store.lock")).inode
        }
    );
    assert!(acquire(&fixture.0, Some(expected)).is_ok());
}

#[test]
fn only_store_lock_is_proposed_and_null_binding_survives_creation() {
    let fixture = Fixture::new();
    make_incompatible(&fixture.0);
    fs::write(fixture.0.join("store.lock"), b"distinct bytes").unwrap();
    assert_eq!(
        proposal_lock(&fixture.0).unwrap().ino,
        identity(&fixture.0.join("store.lock")).inode
    );

    let absent = Fixture::new();
    make_incompatible(&absent.0);
    assert_eq!(proposal_lock(&absent.0), None);
    assert!(acquire(&absent.0, None).is_ok());
}

#[test]
fn legacy_lock_without_store_lock_does_not_become_the_proposal_binding() {
    let fixture = Fixture::new();
    make_incompatible(&fixture.0);
    fs::write(fixture.0.join(".harflow-store.lock"), b"legacy bytes").unwrap();
    assert_eq!(proposal_lock(&fixture.0), None);
}

#[test]
fn null_proposal_remains_compatible_with_a_later_store_lock() {
    let fixture = Fixture::new();
    make_incompatible(&fixture.0);
    assert_eq!(proposal_lock(&fixture.0), None);
    fs::write(fixture.0.join("store.lock"), b"first").unwrap();
    let old = fixture.0.join("store.lock");
    fs::remove_file(&old).unwrap();
    fs::write(&old, b"replacement").unwrap();
    assert!(acquire(&fixture.0, None).is_ok());
}

#[test]
fn only_store_lock_proposal_rejects_a_replaced_store_lock() {
    let fixture = Fixture::new();
    make_incompatible(&fixture.0);
    let path = fixture.0.join("store.lock");
    fs::write(&path, b"first").unwrap();
    let old_identity = identity(&path);
    let _held_old_inode = fs::File::open(&path).unwrap();
    let proposed = proposal_lock(&fixture.0);
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"replacement").unwrap();
    assert_ne!(identity(&path), old_identity);
    assert!(matches!(
        acquire(&fixture.0, proposed),
        Err(Error::Replaced)
    ));
}

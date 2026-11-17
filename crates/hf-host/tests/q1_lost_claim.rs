#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Simulated media rollback pins the accepted EarlierSnapshot live-orphan residual.
use hf_application::ctl::{
    state_inspect::{OperationIdPort, ProposalPorts, StorePolicy, inspect_state},
    state_recover::flow::{RecoveryPorts, RecoveryRequest, archive_only as recover},
};
use hf_domain::ctl::state_root::StateRoot;
use hf_interfaces::ctl::{
    recovery_codec::JsonRecoveryCodec, recovery_journal::JsonJournalCodec,
    state_codec::JsonStateCodec,
};
use hf_platform::ctl::{
    archive_fs::{FsArchive, MacExclusiveRename, NoArchiveFault},
    inventory::FsInventory,
    recovery_fs::MacRecoveryFs,
    state_observer::FsStateObserver,
};
use hf_storage::{Limits, MaintenanceSession, Mutation, SessionRequest, Sha256Digest, Store};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "q1_lost_claim/child.rs"]
mod child;
#[allow(dead_code)]
#[path = "state_recover_flow/completion_support.rs"]
mod completion_support;
#[allow(dead_code)]
#[path = "state_recover_flow/support.rs"]
mod support;

use hf_application::{
    ctl::state_recover::evidence::{Basis, OwnerEvidencePort, Refusal, Subject, assess},
    owner_runtime::DigestPort,
    process::IdentityPort,
};
use hf_domain::ownership::{ClaimResult, Id, OwnerClaim, PreparedRequest};
use hf_platform::process::MacIdentity;
use support::*;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(8)
}
fn open(root: &Path) -> Store {
    Store::open(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap()
}
fn id(n: u8) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}

#[test]
fn q1_lost_acknowledged_claim_with_live_orphan() {
    // Never automatically remove this root on failure: the child may remain unobserved.
    let fixture = std::mem::ManuallyDrop::new(Fixture::new());
    let f = &*fixture;
    let store = Store::initialize_fixture(
        &f.0,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let request = PreparedRequest {
        operation: id(1),
        session: id(2),
        reservation: id(3),
        attempt: id(4),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Sha256Digest.digest_parts(&[b"Q1 fixture"]),
        payload: b"Q1 fixture".to_vec(),
    };
    drop(store);
    // A quiescent, clean pre-prepare image, including the WAL. No snapshot reader remains.
    let old_db = fs::read(f.0.join("store.sqlite3")).unwrap();
    let old_wal = fs::read(f.0.join("store.sqlite3-wal")).unwrap();
    let old_watermark = fs::read(f.0.join("store.watermark")).unwrap();
    acknowledge_claim(f, &request);
    fs::write(f.0.join("payload"), b"before archive").unwrap();
    let payload_identity = file_identity(&f.0.join("payload"));
    // No Store or owner lease exists at spawn; Python opens only payload and IPC.
    let mut child = child::Controlled::spawn(&f.0.join("payload"));
    let identity = MacIdentity.identity(child.pid()).unwrap();
    let store = open(&f.0);
    let claimed = store
        .ownership_observe(&request.operation, deadline())
        .unwrap()
        .unwrap();
    let recorded = store
        .ownership_record_spawn(&claimed.fence().unwrap(), &identity, deadline())
        .unwrap();
    assert_eq!(recorded.child.as_ref(), Some(&identity));
    assert_eq!(
        store
            .ownership_observe(&request.operation, deadline())
            .unwrap(),
        Some(recorded)
    );
    drop(store);
    let newer_watermark = fs::read(f.0.join("store.watermark")).unwrap();
    assert_ne!(old_watermark, newer_watermark);
    known_claim_blocks(f, &identity);

    rollback(f, old_db, old_wal, newer_watermark);
    recover_live_orphan(f, &request, &identity, payload_identity, &mut child);
    fs::remove_dir_all(f.0.parent().unwrap()).unwrap();
}

fn rollback(f: &Fixture, old_db: Vec<u8>, old_wal: Vec<u8>, newer_watermark: Vec<u8>) {
    // Explicit simulated media rollback, not a real fault or W1 qualification:
    // only DB/WAL revert; the acknowledged watermark and payload remain current.
    fs::write(f.0.join("store.sqlite3"), old_db).unwrap();
    fs::write(f.0.join("store.sqlite3-wal"), old_wal).unwrap();
    assert_eq!(
        fs::read(f.0.join("store.watermark")).unwrap(),
        newer_watermark
    );
    assert!(matches!(
        Store::open(
            &f.0,
            Limits::default(),
            deadline(),
            hf_platform::lease::shared(),
        ),
        Err(hf_storage::Error::CommitLost)
    ));
}

fn recover_live_orphan(
    f: &Fixture,
    request: &PreparedRequest,
    identity: &hf_domain::process::ProcessIdentity,
    payload_identity: hf_domain::capture::FileIdentity,
    child: &mut child::Controlled,
) {
    let proposal = f.proposal();
    let session = f.session(&proposal);
    let evidence = session.read(&proposal.operation_id).unwrap();
    assert_eq!(evidence.basis, Basis::EarlierSnapshot);
    assert!(
        evidence.items.is_empty(),
        "pre-prepare snapshot has no surviving claim"
    );
    assert!(assess(&evidence, &MacIdentity).unwrap().checked.is_empty());
    child.assert_alive(identity);
    f.complete(
        &proposal,
        &session,
        &NoArchiveFault,
        &session,
        &hf_storage::RecoveryInitializer::new(&session, &hf_platform::durability::FullFlush),
    )
    .unwrap();
    let archived = PathBuf::from(proposal.backup_path()).join("data/payload");
    assert_eq!(file_identity(&archived), payload_identity);
    assert!(!f.0.join("payload").exists());
    child.assert_alive(identity);
    drop(session);
    let store = open(&f.0);
    assert!(
        store
            .ownership_observe(&request.operation, deadline())
            .unwrap()
            .is_none()
    );
    drop(store);
    let new_root = inventory(&f.0);
    child.release_and_reap();
    assert_eq!(
        fs::read(&archived).unwrap(),
        b"before archive|Q1-after-archive-only|"
    );
    assert_eq!(file_identity(&archived), payload_identity);
    assert!(!f.0.join("payload").exists());
    // Exact byte/inode inventory comparison of the fresh root after the orphan write.
    assert_eq!(inventory(&f.0), new_root);
}

fn acknowledge_claim(f: &Fixture, request: &PreparedRequest) {
    let store = open(&f.0);
    store.ownership_prepare(request, deadline()).unwrap();
    let owner = OwnerClaim {
        owner: id(5),
        nonce: id(6),
        process: MacIdentity.identity(std::process::id()).unwrap(),
    };
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner,
            deadline(),
        )
        .unwrap()
    else {
        panic!("actual durable Granted acknowledgement required")
    };
    assert_eq!(
        store
            .ownership_observe(&request.operation, deadline())
            .unwrap(),
        Some(claimed)
    );
}

fn known_claim_blocks(f: &Fixture, identity: &hf_domain::process::ProcessIdentity) {
    // A session can inspect a ready root even though there is no recovery proposal yet.
    let session = MaintenanceSession::acquire_within(
        &f.0,
        &SessionRequest {
            root_identity: file_identity(&f.0),
            lock_identity: Some(file_identity(&f.0.join("store.lock"))),
        },
        Arc::new(MacRecoveryFs),
        hf_platform::lease::shared(),
        Duration::from_secs(2),
    )
    .unwrap();
    let evidence = session.read("0123456789abcdef0123456789abcdef").unwrap();
    assert_eq!(evidence.basis, Basis::Clear);
    assert!(
        evidence
            .items
            .iter()
            .any(|item| item.subject == Subject::Identity(identity.clone()))
    );
    assert_eq!(assess(&evidence, &MacIdentity), Err(Refusal::ActiveOwner));
}

fn inventory(root: &Path) -> String {
    let mut entries = originals(root);
    entries.sort_by(|a, b| a.relative.cmp(&b.relative));
    let infrastructure: Vec<_> = hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED
        .iter()
        .filter_map(|name| {
            let path = root.join(name);
            path.exists()
                .then(|| (*name, file_identity(&path), fs::read(path).unwrap()))
        })
        .collect();
    format!("{entries:?}|{infrastructure:?}")
}

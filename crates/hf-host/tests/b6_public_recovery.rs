#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
//! B6 system boundary: real ctl subprocesses resume private fault checkpoints.
use hf_application::ctl::{
    state_inspect::{OperationIdPort, ProposalPorts, StorePolicy, inspect_state},
    state_recover::flow::{RecoveryPorts, RecoveryRequest, archive_only as recover},
};
use hf_domain::ctl::state_root::StateRoot;
use hf_interfaces::ctl::{
    recovery_codec::{JsonRecoveryCodec, encode_token},
    recovery_journal::JsonJournalCodec,
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
#[allow(dead_code)]
#[path = "state_recover_flow/completion_support.rs"]
mod completion_support;
#[allow(dead_code)]
#[path = "state_recover_flow/support.rs"]
mod support;

fn ctl(
    f: &support::Fixture,
    p: &hf_domain::ctl::recovery_proposal::RecoveryProposal,
) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_harflow-ctl"))
        .args([
            "state-recover",
            "--operation-id",
            &p.operation_id,
            "--confirmation-token",
            &encode_token(p),
            "--json",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HARFLOW_STATE_ROOT", &f.0)
        .output()
        .unwrap()
}
fn success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("\"status\": \"completed\""));
}

#[test]
fn public_ctl_resumes_backed_up_and_preserves_original_archive() {
    let _serial = support::serial();
    let f = support::Fixture::new();
    f.seed();
    let proposal = f.proposal();
    let items = support::originals(&f.0);
    let session = f.session(&proposal);
    f.run(&proposal, &session, &NoArchiveFault, &session)
        .unwrap();
    drop(session);
    success(&ctl(&f, &proposal));
    archived_originals(&f.0, Path::new(&proposal.backup_path()), &items);
    let store = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    drop(store);
}

// B4's name-location assertion ends at BackedUp. R8 deliberately reuses
// store filenames for a fresh generation; qualify the original archived inode.
fn archived_originals(root: &Path, backup: &Path, items: &[support::Original]) {
    for item in items {
        let archived = backup.join("data").join(&item.relative);
        assert_eq!(support::file_identity(&archived), item.identity);
        if let Some(bytes) = &item.bytes {
            assert_eq!(&fs::read(&archived).unwrap(), bytes);
        }
        if let Some(link) = &item.link {
            assert_eq!(&fs::read_link(&archived).unwrap(), link);
        }
        let current = root.join(&item.relative);
        if fs::symlink_metadata(&current).is_ok() {
            assert_ne!(support::file_identity(&current), item.identity);
        }
    }
}

struct ReceiptFault;
impl hf_platform::ctl::archive_fs::ArchiveFaultPort for ReceiptFault {
    fn before(&self, name: &'static str) -> std::io::Result<()> {
        if name == "receipt.rename" {
            Err(std::io::Error::other("B6 receipt fault"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn public_unfinished_receipt_requires_exclusion_then_history_bypasses_live_store_lease() {
    let _serial = support::serial();
    let f = support::Fixture::new();
    f.seed();
    let p = f.proposal();
    let session = f.session(&p);
    {
        let initializer = session.initializer(&hf_platform::durability::FullFlush);
        assert!(
            f.complete(&p, &session, &ReceiptFault, &session, &initializer)
                .is_err()
        );
    }
    drop(session);
    let live = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    live.apply_batch(
        &[Mutation::Put {
            key: "after-r8".into(),
            value: b"retained".to_vec(),
        }],
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    fs::write(f.0.join("after-r8-sentinel"), b"preserve").unwrap();
    let current: Vec<_> = ["store.sqlite3", "store.sqlite3-wal", "store.watermark"]
        .into_iter()
        .map(|name| (name, fs::read(f.0.join(name)).unwrap()))
        .collect();
    let blocked = ctl(&f, &p);
    assert_eq!(blocked.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&blocked.stdout).contains("\"reason\": \"active_owner\""));
    for (name, bytes) in &current {
        assert_eq!(&fs::read(f.0.join(name)).unwrap(), bytes);
    }
    assert!(!Path::new(&p.backup_path()).join("receipt.json").exists());
    drop(live);
    let first = ctl(&f, &p);
    success(&first);
    let live = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let repeated = ctl(&f, &p);
    success(&repeated);
    assert_eq!(first.stdout, repeated.stdout);
    for (name, bytes) in current {
        assert_eq!(fs::read(f.0.join(name)).unwrap(), bytes);
    }
    assert_eq!(
        fs::read(f.0.join("after-r8-sentinel")).unwrap(),
        b"preserve"
    );
    drop(live);
    historical_after_later_recovery(&f, &p, &first.stdout);
}

fn historical_after_later_recovery(
    f: &support::Fixture,
    p: &hf_domain::ctl::recovery_proposal::RecoveryProposal,
    first_stdout: &[u8],
) {
    // A later operation archives the old journal; the older receipt stays lease-free.
    let generation = fs::read_to_string(f.0.join("store.identity"))
        .unwrap()
        .lines()
        .nth(2)
        .unwrap()
        .to_owned();
    fs::write(
        f.0.join("store.fence"),
        format!("harflow-fence-1\n{generation}\nfailed\n"),
    )
    .unwrap();
    let mut next = f.proposal();
    next.operation_id = "fedcba9876543210fedcba9876543210".into();
    assert_ne!(p.operation_id, next.operation_id);
    success(&ctl(f, &next));
    let live = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    // Once the final old receipt exists, unrelated current-journal metadata
    // is not an input to the historical operation.
    fs::write(
        f.0.join(".harflow-recovery.json"),
        b"{unrelated malformed journal",
    )
    .unwrap();
    let journal_before = fs::read(f.0.join(".harflow-recovery.json")).unwrap();
    let historical = ctl(f, p);
    success(&historical);
    assert_eq!(historical.stdout, first_stdout);
    assert_eq!(
        fs::read(f.0.join(".harflow-recovery.json")).unwrap(),
        journal_before
    );
    drop(live);
}

#[test]
fn public_fresh_recovery_refuses_real_store_owner_before_archive() {
    let _serial = support::serial();
    let f = support::Fixture::new();
    f.seed();
    let fence = fs::read(f.0.join("store.fence")).unwrap();
    fs::remove_file(f.0.join("store.fence")).unwrap();
    let live = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    fs::write(f.0.join("store.fence"), fence).unwrap();
    let p = f.proposal();
    let result = ctl(&f, &p);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stdout).contains("\"reason\": \"active_owner\""));
    assert!(!Path::new(&p.backup_path()).exists());
    assert!(!f.0.join(".harflow-recovery.json").exists());
    assert_eq!(
        fs::read(f.0.join("nested/payload")).unwrap(),
        b"original payload\0\xff"
    );
    drop(live);
}

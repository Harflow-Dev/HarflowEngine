#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "archive_fs/support.rs"]
mod support;
use hf_application::ctl::state_recover::flow::ArchiveMutationPort;
use hf_domain::ctl::recovery_proposal::{EntryBody, EntryStat, InventoryEntry};
use hf_platform::ctl::archive_fs::ArchiveFaultPort;
use std::{fs, io, os::unix::fs::MetadataExt, path::PathBuf};
use support::{Fault, Fixture, OP, RecordingFault};

#[test]
fn move_keeps_exact_bytes_and_inode_and_retry_after_sync_fault_is_idempotent() {
    let f = Fixture::new();
    let before = fs::metadata(f.root.join("store.identity")).unwrap();
    let expected = &f.journal.inventory.entries["store.identity"];
    assert_eq!(
        f.archive(&Fault {
            boundary: Some("entry.root_sync")
        })
        .move_entry(&f.journal, "store.identity", expected),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable)
    );
    assert!(!f.root.join("store.identity").exists());
    assert_eq!(fs::read(f.data.join("store.identity")).unwrap(), f.original);
    let after = fs::metadata(f.data.join("store.identity")).unwrap();
    assert_eq!(before.ino(), after.ino());
    f.archive(&Fault { boundary: None })
        .move_entry(&f.journal, "store.identity", expected)
        .unwrap();
    assert_eq!(fs::read(f.data.join("store.identity")).unwrap(), f.original);
}

#[test]
fn every_regular_entry_move_boundary_preserves_one_exact_inode_across_retry() {
    for boundary in ["entry.rename", "entry.root_sync", "entry.data_sync"] {
        let f = Fixture::new();
        let before = fs::metadata(f.root.join("store.identity")).unwrap();
        let expected = &f.journal.inventory.entries["store.identity"];
        let faults = RecordingFault {
            fail_once: Some(boundary),
            ..RecordingFault::default()
        };
        assert_eq!(
            f.archive(&faults)
                .move_entry(&f.journal, "store.identity", expected),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable),
            "boundary {boundary}"
        );
        f.archive(&faults)
            .move_entry(&f.journal, "store.identity", expected)
            .unwrap();
        assert!(!f.root.join("store.identity").exists());
        let archived = f.data.join("store.identity");
        let after = fs::metadata(&archived).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(fs::read(archived).unwrap(), f.original);
    }
}

#[test]
fn shm_rename_and_sync_faults_retain_exactly_one_sidecar() {
    for boundary in ["shm.rename", "shm.root_sync", "shm.data_sync"] {
        let f = Fixture::new();
        let source = f.root.join("store.sqlite3-shm");
        let bytes = b"sidecar exact bytes";
        fs::write(&source, bytes).unwrap();
        let before = fs::metadata(&source).unwrap();
        let faults = RecordingFault {
            fail_once: Some(boundary),
            ..RecordingFault::default()
        };
        assert_eq!(
            f.archive(&faults).move_shm(&f.journal),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable),
            "boundary {boundary}"
        );
        f.archive(&faults).move_shm(&f.journal).unwrap();
        assert!(!source.exists());
        let archived = f.data.join("store.sqlite3-shm");
        let after = fs::metadata(&archived).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(fs::read(archived).unwrap(), bytes);
    }
}

#[test]
fn occupied_destination_is_never_overwritten_or_deleted() {
    let f = Fixture::new();
    let collision = b"preexisting backup value\n";
    fs::write(f.data.join("store.identity"), collision).unwrap();
    let expected = &f.journal.inventory.entries["store.identity"];
    assert_eq!(
        f.archive(&Fault { boundary: None })
            .move_entry(&f.journal, "store.identity", expected),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
    );
    assert_eq!(fs::read(f.root.join("store.identity")).unwrap(), f.original);
    assert_eq!(fs::read(f.data.join("store.identity")).unwrap(), collision);
}

#[test]
fn occupied_symlink_and_empty_directory_destinations_are_never_replaced() {
    for directory in [false, true] {
        let f = Fixture::new();
        let destination = f.data.join("store.identity");
        if directory {
            fs::create_dir(&destination).unwrap();
        } else {
            std::os::unix::fs::symlink("unrelated-target", &destination).unwrap();
        }
        let before = fs::symlink_metadata(&destination).unwrap();
        let expected = &f.journal.inventory.entries["store.identity"];
        assert_eq!(
            f.archive(&Fault { boundary: None })
                .move_entry(&f.journal, "store.identity", expected),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
        );
        assert!(f.root.join("store.identity").exists());
        let after = fs::symlink_metadata(destination).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(before.file_type().is_dir(), after.file_type().is_dir());
        if !directory {
            assert_eq!(
                fs::read_link(f.data.join("store.identity")).unwrap(),
                PathBuf::from("unrelated-target")
            );
        }
    }
}

#[test]
fn actual_exclusive_rename_port_rejects_invalid_leaf_names() {
    use hf_platform::ctl::archive_fs::{ExclusiveRenamePort, MacExclusiveRename};
    use std::os::fd::OwnedFd;

    let f = Fixture::new();
    let source = fs::File::open(&f.root).unwrap();
    let destination = fs::File::open(&f.data).unwrap();
    let source: OwnedFd = source.into();
    let destination: OwnedFd = destination.into();
    for name in ["", ".", "..", "a/b", "nul\0byte"] {
        let error = MacExclusiveRename
            .rename_exclusive(&source, name, &destination, "target")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name:?}");
    }
    assert!(f.root.join("store.identity").exists());
}

#[test]
fn real_rename_archives_symlink_without_following_or_changing_its_inode() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    let target = f.root.join("outside-target");
    fs::write(&target, b"leave target untouched").unwrap();
    let link = f.root.join("state-link");
    symlink(&target, &link).unwrap();
    let before = fs::symlink_metadata(&link).unwrap();
    let target_text = fs::read_link(&link).unwrap();
    let entry = InventoryEntry {
        stat: stat(&before),
        body: EntryBody::Link(target_text.to_string_lossy().into_owned()),
    };
    f.archive(&Fault { boundary: None })
        .move_entry(&f.journal, "state-link", &entry)
        .unwrap();
    let archived = f.data.join("state-link");
    let after = fs::symlink_metadata(&archived).unwrap();
    assert!(after.file_type().is_symlink());
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_eq!(fs::read_link(archived).unwrap(), target_text);
    assert_eq!(fs::read(&target).unwrap(), b"leave target untouched");
}

#[test]
fn real_rename_archives_directory_tree_as_one_inode_preserving_entry() {
    let f = Fixture::new();
    let directory = f.root.join("state-dir");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("child"), b"child bytes").unwrap();
    let before = fs::symlink_metadata(&directory).unwrap();
    let child = fs::symlink_metadata(directory.join("child")).unwrap();
    let entry = InventoryEntry {
        stat: stat(&before),
        body: EntryBody::Directory(
            [(
                "child".into(),
                InventoryEntry {
                    stat: stat(&child),
                    body: EntryBody::File,
                },
            )]
            .into(),
        ),
    };
    f.archive(&Fault { boundary: None })
        .move_entry(&f.journal, "state-dir", &entry)
        .unwrap();
    let archived = f.data.join("state-dir");
    let after = fs::symlink_metadata(&archived).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_eq!(fs::read(archived.join("child")).unwrap(), b"child bytes");
    assert!(!directory.exists());
}

fn stat(metadata: &fs::Metadata) -> EntryStat {
    EntryStat {
        dev: metadata.dev() as i64,
        ino: metadata.ino(),
        mode: metadata.mode(),
        size: metadata.len() as i64,
        mtime_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
    }
}

#[test]
fn destination_collision_created_at_rename_boundary_preserves_both_files() {
    struct CollisionFault<'a> {
        data: &'a PathBuf,
    }
    impl ArchiveFaultPort for CollisionFault<'_> {
        fn before(&self, boundary: &'static str) -> io::Result<()> {
            if boundary == "entry.rename" {
                fs::write(self.data.join("store.identity"), b"racing occupant")?;
            }
            Ok(())
        }
    }
    let f = Fixture::new();
    let expected = &f.journal.inventory.entries["store.identity"];
    assert_eq!(
        f.archive(&CollisionFault { data: &f.data }).move_entry(
            &f.journal,
            "store.identity",
            expected
        ),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
    );
    assert_eq!(fs::read(f.root.join("store.identity")).unwrap(), f.original);
    assert_eq!(
        fs::read(f.data.join("store.identity")).unwrap(),
        b"racing occupant"
    );
}

#[test]
fn retry_repeats_parent_sync_after_backup_directory_was_created() {
    let mut f = Fixture::new();
    f.reset_backup_for_prepare();
    let fault = RecordingFault {
        fail_once: Some("backup.parent_sync"),
        ..RecordingFault::default()
    };
    assert_eq!(
        f.archive(&fault).prepare_backup(&f.journal, &f.codec),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable)
    );
    f.archive(&fault)
        .prepare_backup(&f.journal, &f.codec)
        .unwrap();
    let seen = fault.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|&&name| name == "backup.parent_sync")
            .count(),
        2
    );
    assert!(f.root.join("store.identity").exists());
}

#[test]
fn retry_repeats_previous_journal_sync_for_exact_existing_bytes() {
    let mut f = Fixture::new();
    f.reset_backup_for_prepare();
    let previous = b"exact earlier journal bytes\n".to_vec();
    f.journal.previous_journal_bytes = Some(previous.clone());
    f.journal.proposal.prior_journal_sha256 = Some("b".repeat(64));
    let fault = RecordingFault {
        fail_once: Some("previous.directory_sync"),
        ..RecordingFault::default()
    };
    assert_eq!(
        f.archive(&fault).prepare_backup(&f.journal, &f.codec),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable)
    );
    assert_eq!(
        fs::read(PathBuf::from(&f.journal.backup_path).join("previous-recovery.json")).unwrap(),
        previous
    );
    f.archive(&fault)
        .prepare_backup(&f.journal, &f.codec)
        .unwrap();
    let seen = fault.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|&&name| name == "previous.directory_sync")
            .count(),
        2
    );
    assert!(f.root.join("store.identity").exists());
}

#[test]
fn malformed_publication_temporary_is_preserved_and_refuses_retry() {
    let f = Fixture::new();
    let fault = Fault {
        boundary: Some("file.write"),
    };
    assert_eq!(
        f.archive(&fault).write_pending(&f.journal),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::InfrastructureUnavailable)
    );
    let temporary = f.root.join("store.recovery.tmp");
    let partial = fs::read(&temporary).unwrap();
    assert_eq!(
        f.archive(&Fault { boundary: None })
            .write_pending(&f.journal),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
    );
    assert_eq!(fs::read(temporary).unwrap(), partial);
}

#[test]
fn malformed_final_recovery_record_is_replaced_by_the_pending_gate() {
    let f = Fixture::new();
    let final_record = f.root.join("store.recovery");
    fs::write(&final_record, b"malformed but bounded\n").unwrap();
    f.archive(&Fault { boundary: None })
        .write_pending(&f.journal)
        .unwrap();
    let bytes = fs::read(final_record).unwrap();
    assert_eq!(
        hf_domain::ctl::state_recovery::StoreRecovery::decode(&bytes)
            .unwrap()
            .operation_id(),
        OP
    );
    assert_eq!(
        hf_domain::ctl::state_recovery::StoreRecovery::decode(&bytes)
            .unwrap()
            .state(),
        hf_domain::ctl::state_recovery::RecoveryState::Pending
    );
}

#[test]
fn publication_refuses_if_final_record_changes_during_the_hook() {
    struct RewriteFinal<'a> {
        root: &'a PathBuf,
    }
    impl ArchiveFaultPort for RewriteFinal<'_> {
        fn before(&self, boundary: &'static str) -> io::Result<()> {
            if boundary == "publication.rename" {
                fs::write(self.root.join("store.recovery"), b"racing final value")?;
            }
            Ok(())
        }
    }
    let f = Fixture::new();
    let final_record = f.root.join("store.recovery");
    fs::write(&final_record, b"initial malformed value").unwrap();
    assert_eq!(
        f.archive(&RewriteFinal { root: &f.root })
            .write_pending(&f.journal),
        Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
    );
    assert_eq!(fs::read(final_record).unwrap(), b"racing final value");
    assert!(f.root.join("store.recovery.tmp").exists());
}

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! What each checkpoint policy does to an existing root at open and close, and that a
//! read session never checkpoints even with an Owner writer and a long log.
mod common;
mod wal_owner_support;
use common::{Fixture, deadline, default_limits, durable_tree};
use hf_storage::{ReadSession, Store, WalMaintenance, WriterOptions};
use wal_owner_support::{MAIN, WAL, bulk, bytes, initialized, open, owner, pin, put, release};

/// An initialized root with three small commits left in `-wal` by a Suppressed writer.
fn seeded(scratch: &Fixture) -> std::path::PathBuf {
    let root = initialized(scratch);
    let store = open(&root, WriterOptions::suppressed());
    for fill in 0..3 {
        put(&store, &format!("key-{fill}"), 64, fill);
    }
    drop(store);
    root
}

fn assert_regular_nonempty_wal(root: &std::path::Path, label: &str) {
    let metadata = std::fs::symlink_metadata(root.join(WAL)).unwrap();
    assert!(
        metadata.file_type().is_file(),
        "{label}: -wal is not regular"
    );
    assert!(metadata.len() > 0, "{label}: -wal was emptied");
}

#[test]
fn the_open_time_checkpoint_keeps_the_wal_and_the_last_owner_close_leaves_it() {
    let scratch = Fixture::new();
    let root = seeded(&scratch);
    let wal = bytes(&root, WAL);
    let main = bytes(&root, MAIN);

    let store = owner(&root);
    assert_regular_nonempty_wal(&root, "after open");
    assert_eq!(bytes(&root, WAL), wal, "PASSIVE never rewrites -wal");
    assert_ne!(bytes(&root, MAIN), main, "the open-time PASSIVE ran");
    assert!(store.checkpoint_passive(deadline()).unwrap().complete);
    drop(store);

    assert_regular_nonempty_wal(&root, "after the last owner close");
    assert_eq!(
        bytes(&root, WAL),
        wal,
        "close neither checkpoints nor truncates"
    );
    for _ in 0..2 {
        drop(owner(&root));
        assert_regular_nonempty_wal(&root, "after a repeated owner cycle");
    }
    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let records = reopened.read_batch(&["key-2".into()], deadline()).unwrap();
    assert_eq!(records[0].value, vec![2; 64]);
}

#[test]
fn suppressed_open_maintain_and_close_leave_a_seeded_root_byte_identical() {
    let scratch = Fixture::new();
    let root = seeded(&scratch);
    let before = durable_tree(&root);
    let shorthand = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(
        shorthand.writer_options().checkpoint,
        hf_storage::CheckpointPolicy::Suppressed
    );
    drop(shorthand);
    for _ in 0..2 {
        let store = open(&root, WriterOptions::suppressed());
        assert_eq!(
            store.maintain_wal(deadline()).unwrap(),
            WalMaintenance::Idle
        );
        let records = store.read_batch(&["key-0".into()], deadline()).unwrap();
        assert_eq!(records[0].value, vec![0; 64]);
        drop(store);
    }
    assert_eq!(durable_tree(&root), before);
}

#[test]
fn a_read_session_never_checkpoints_beside_an_owner_with_a_long_log() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    let reader = pin(&root);
    bulk(&store, 9);
    release(reader);
    let main = bytes(&root, MAIN);
    let wal = bytes(&root, WAL);

    for _ in 0..2 {
        let session = ReadSession::open(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap();
        session.snapshot(deadline(), |_| Ok(())).unwrap();
        drop(session);
    }
    assert_eq!(bytes(&root, MAIN), main, "a read session copied frames");
    assert_eq!(bytes(&root, WAL), wal, "a read session changed -wal");

    assert!(matches!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Checkpointed { complete: true, .. }
    ));
    assert_ne!(
        bytes(&root, MAIN),
        main,
        "the backlog was the owner's to copy"
    );
}

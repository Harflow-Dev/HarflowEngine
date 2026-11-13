#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The Owner checkpoint policy bounds `-wal`: maintenance after each commit, the
//! reader-pinned shrink, and the skip outcomes that leave the root untouched.
mod common;
mod wal_owner_support;
use common::{Fixture, deadline};
use hf_storage::{JOURNAL_SIZE_LIMIT, PASSIVE_AFTER, Store, WalMaintenance, WriterOptions};
use std::time::{Duration, Instant};
use wal_owner_support::{
    BULK, MAIN, ONE_COMMIT, bulk, bytes, initialized, open, owner, pin, put, release, value,
    wal_len,
};

const DONE: WalMaintenance = WalMaintenance::Checkpointed {
    complete: true,
    shrink_commit_landed: false,
};
const SHRUNK: WalMaintenance = WalMaintenance::Checkpointed {
    complete: true,
    shrink_commit_landed: true,
};

/// `count` bulk commits while a raw reader pins the log, so nothing is backfilled.
fn pinned_fill(store: &Store, root: &std::path::Path, count: u8) {
    let reader = pin(root);
    bulk(store, count);
    release(reader);
}

#[test]
fn sustained_owner_commits_keep_the_wal_bounded_without_a_reader() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    let mut largest = 0;
    for fill in 0..40u8 {
        put(&store, "bulk", BULK, fill);
        let length = wal_len(&root);
        assert!(
            length <= PASSIVE_AFTER + 2 * ONE_COMMIT,
            "commit {fill}: -wal is {length} bytes"
        );
        largest = largest.max(length);
    }
    assert!(
        largest > PASSIVE_AFTER,
        "the log must have reached the trigger: {largest}"
    );
    assert_eq!(value(&store, "bulk"), vec![39; BULK]);
}

#[test]
fn suppressed_commits_grow_past_the_limit_and_maintenance_stays_idle() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = open(&root, WriterOptions::suppressed());
    bulk(&store, 18);
    let length = wal_len(&root);
    assert!(length > JOURNAL_SIZE_LIMIT, "{length}");
    let main = bytes(&root, MAIN);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    assert_eq!(bytes(&root, MAIN), main, "Suppressed never checkpoints");
    assert_eq!(wal_len(&root), length);
}

#[test]
fn a_pinned_wal_grows_then_shrinks_to_the_limit_after_a_skipped_maintenance() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    pinned_fill(&store, &root, 18);
    let grown = wal_len(&root);
    assert!(
        grown > JOURNAL_SIZE_LIMIT,
        "the reader pinned growth: {grown}"
    );

    let main = bytes(&root, MAIN);
    assert_eq!(
        store.maintain_wal(Instant::now()).unwrap(),
        WalMaintenance::Skipped,
        "an exhausted budget skips"
    );
    assert_eq!(bytes(&root, MAIN), main, "a skip changes nothing");
    assert_eq!(wal_len(&root), grown);

    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_eq!(wal_len(&root), JOURNAL_SIZE_LIMIT);
    assert_eq!(value(&store, "bulk"), vec![17; BULK]);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );

    put(&store, "bulk", BULK, 99);
    assert!(wal_len(&root) <= JOURNAL_SIZE_LIMIT);
    assert_eq!(value(&store, "bulk"), vec![99; BULK]);
}

#[test]
fn a_busy_maintenance_commit_reports_no_shrink_and_a_later_call_shrinks() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    pinned_fill(&store, &root, 18);
    assert!(store.checkpoint_passive(deadline()).unwrap().complete);
    let grown = wal_len(&root);
    assert!(grown > JOURNAL_SIZE_LIMIT, "{grown}");

    let writer = common::raw_connection(root.join(MAIN));
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let budget = Instant::now() + Duration::from_millis(400);
    assert_eq!(store.maintain_wal(budget).unwrap(), DONE);
    assert_eq!(wal_len(&root), grown, "no maintenance write landed");
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);

    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_eq!(wal_len(&root), JOURNAL_SIZE_LIMIT);
}

#[test]
fn a_held_io_lock_skips_maintenance_without_a_change() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    pinned_fill(&store, &root, 9);
    let main = bytes(&root, MAIN);
    let length = wal_len(&root);
    {
        let gate = std::fs::File::open(root.join("io.lock")).unwrap();
        gate.try_lock().unwrap();
        assert_eq!(
            store.maintain_wal(deadline()).unwrap(),
            WalMaintenance::Skipped
        );
        assert_eq!(bytes(&root, MAIN), main);
        assert_eq!(wal_len(&root), length);
    }
    assert_eq!(store.maintain_wal(deadline()).unwrap(), DONE);
    assert_ne!(
        bytes(&root, MAIN),
        main,
        "the released lock lets PASSIVE run"
    );
}

#[test]
fn a_short_live_log_is_idle_even_when_the_wal_file_is_longer() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    pinned_fill(&store, &root, 9);
    assert_eq!(store.maintain_wal(deadline()).unwrap(), DONE);
    put(&store, "small", 16, 1);
    let length = wal_len(&root);
    assert!(
        length > PASSIVE_AFTER,
        "the restarted log keeps the file length: {length}"
    );
    let main = bytes(&root, MAIN);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    assert_eq!(bytes(&root, MAIN), main);
    assert_eq!(wal_len(&root), length);
}

#[test]
fn shrink_and_idle_outcomes_keep_every_committed_value() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner(&root);
    put(&store, "kept", 32, 7);
    pinned_fill(&store, &root, 18);
    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    drop(store);
    let reopened = Store::open(
        &root,
        common::default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(value(&reopened, "kept"), vec![7; 32]);
    assert_eq!(value(&reopened, "bulk"), vec![17; BULK]);
}

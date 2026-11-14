#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The slot rule of the commit watermark: a commit writes the slot that does not hold
//! the best valid entry, so the best entry survives any torn write, and a database rolled
//! back to an acknowledged sequence stays readable while one below it is `CommitLost`.
mod common;
mod watermark_support;
use hf_application::watermark::{self, Decoded, Slot};
use hf_storage::{Error, Store};
use std::sync::Arc;
use watermark_support::{
    Recorder, at, database_seq, decoded, file_bytes, generation, initialized, put, reopen,
    restore_database, save_database, session, set_slot, slot_block, tear_slot, write_file,
};

fn best(decoded: Decoded) -> (Slot, u64) {
    match decoded {
        Decoded::Valid { best, seq } => (best, seq),
        other => panic!("no valid slot: {other:?}"),
    }
}

/// `count` commits on `store`, each with its own key.
fn commits(store: &Store, from: u64, count: u64) {
    for number in from..from + count {
        put(store, &format!("key-{number}")).unwrap();
    }
}

#[test]
fn every_commit_writes_the_other_slot_and_leaves_the_best_one_unchanged() {
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    assert_eq!(
        decoded(root),
        at(Slot::A, 0),
        "equal sequences: slot A is best"
    );
    for number in 1..=6u64 {
        let (old_best, old_seq) = best(decoded(root));
        let kept = slot_block(root, old_best);
        put(&store, &format!("key-{number}")).unwrap();
        assert_eq!(slot_block(root, old_best), kept, "commit {number}");
        assert_eq!(old_seq + 1, number);
        assert_eq!(
            decoded(root),
            at(old_best.other(), number),
            "commit {number}"
        );
        assert_eq!(database_seq(root), number);
    }
}

#[test]
fn with_only_one_valid_slot_the_invalid_slot_is_the_one_written() {
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    put(&store, "one").unwrap();
    assert_eq!(decoded(root), at(Slot::B, 1));
    // The best slot is damaged (media loss): slot A at 0 is the only valid entry.
    tear_slot(root, Slot::B);
    assert_eq!(decoded(root), at(Slot::A, 0));
    let kept = slot_block(root, Slot::A);
    put(&store, "two").unwrap();
    assert_eq!(slot_block(root, Slot::A), kept);
    assert_eq!(decoded(root), at(Slot::B, 2));
    // And the mirror case: slot A invalid, slot B best.
    tear_slot(root, Slot::A);
    let kept = slot_block(root, Slot::B);
    put(&store, "three").unwrap();
    assert_eq!(slot_block(root, Slot::B), kept);
    assert_eq!(decoded(root), at(Slot::A, 3));
}

#[test]
fn a_torn_target_followed_by_two_commits_keeps_a_valid_slot_at_every_point() {
    let port = Arc::new(Recorder::default());
    let (root, store) = initialized(port.clone());
    let root = &root.root;
    commits(&store, 1, 2);
    assert_eq!(decoded(root), at(Slot::A, 2));
    // A publication of commit 3 into slot B was torn before it completed.
    tear_slot(root, Slot::B);
    assert_eq!(decoded(root), at(Slot::A, 2));
    let first = port.images().len();
    commits(&store, 3, 2);
    assert_eq!(decoded(root), at(Slot::A, 4));
    let generation = generation(root);
    let mut last = 2;
    for (index, (ok, image)) in port.images().into_iter().enumerate().skip(first) {
        assert!(ok, "flush {index}");
        let (_, seq) = best(watermark::decode(&image, &generation));
        assert!(seq >= last, "flush {index}: {seq} after {last}");
        last = seq;
    }
    assert_eq!(last, 4);
    drop(store);
    session(root).unwrap();
}

/// Slots 9 and 10; commit 11 lands but its publication is lost; reopen; commit 12, whose
/// publication is torn. Watermark 10 survives, so a database at 10 stays readable and
/// only a database at 9 has lost an acknowledged commit.
#[test]
fn a_lost_then_torn_publication_keeps_ten_readable_and_refuses_nine() {
    let scratch = common::Fixture::new();
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    commits(&store, 1, 9);
    drop(store);
    save_database(root, &scratch.path().join("at-9"));
    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    commits(&store, 10, 1);
    drop(store);
    save_database(root, &scratch.path().join("at-10"));
    assert_eq!(decoded(root), at(Slot::A, 10));
    assert_eq!(slot_block(root, Slot::B), {
        let mut block = vec![0u8; watermark::BLOCK_LEN];
        block[..watermark::PAYLOAD_LEN]
            .copy_from_slice(&watermark::encode_payload(&generation(root), 9).unwrap());
        block
    });

    // Commit 11: the post-write flush fails, and a power loss keeps the pre-flush image.
    let port = Arc::new(Recorder::default());
    let store = reopen(root, port.clone()).unwrap();
    port.fail_after(1);
    assert!(matches!(
        put(&store, "key-11"),
        Err(Error::CommitOutcomeUnknown)
    ));
    drop(store);
    write_file(root, &port.durable().unwrap());
    assert_eq!(decoded(root), at(Slot::A, 10));
    assert_eq!(database_seq(root), 11);

    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    commits(&store, 12, 1);
    drop(store);
    assert_eq!(decoded(root), at(Slot::B, 12));
    tear_slot(root, Slot::B);
    assert_eq!(decoded(root), at(Slot::A, 10));

    restore_database(root, &scratch.path().join("at-10"));
    session(root).unwrap();
    drop(reopen(root, Arc::new(Recorder::default())).unwrap());
    restore_database(root, &scratch.path().join("at-9"));
    assert!(matches!(session(root), Err(Error::CommitLost)));
    assert!(matches!(
        reopen(root, Arc::new(Recorder::default())),
        Err(Error::CommitLost)
    ));
}

/// The documented residual: damage to the newest block lowers the detection threshold to
/// the older entry instead of refusing the root, so a rollback by one commit then passes.
#[test]
fn losing_the_newest_block_lowers_the_threshold_to_the_older_entry() {
    let scratch = common::Fixture::new();
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    commits(&store, 1, 8);
    drop(store);
    save_database(root, &scratch.path().join("at-8"));
    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    commits(&store, 9, 1);
    drop(store);
    save_database(root, &scratch.path().join("at-9"));
    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    commits(&store, 10, 1);
    drop(store);
    assert_eq!(decoded(root), at(Slot::A, 10));
    let intact = file_bytes(root);

    // With both blocks intact, losing commit 10 is refused.
    restore_database(root, &scratch.path().join("at-9"));
    assert!(matches!(session(root), Err(Error::CommitLost)));

    // Damage to the newest block: the threshold drops to 9, so the same loss passes,
    // while a loss of two commits is still refused.
    tear_slot(root, Slot::A);
    assert_eq!(decoded(root), at(Slot::B, 9));
    session(root).unwrap();
    restore_database(root, &scratch.path().join("at-8"));
    assert!(matches!(session(root), Err(Error::CommitLost)));

    // Equal sequences in both slots read as slot A, at that sequence.
    write_file(root, &intact);
    set_slot(root, Slot::B, 10);
    assert_eq!(decoded(root), at(Slot::A, 10));
}

/// Nonzero filler invalidates a block. A commit writes the whole target block, so damaged
/// filler in the non-best slot is repaired and the watermark keeps advancing; a rollback
/// below the latest commit is then still refused.
#[test]
fn damaged_filler_in_the_target_slot_is_repaired_by_the_next_commit() {
    use std::os::unix::fs::FileExt;
    let scratch = common::Fixture::new();
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    commits(&store, 1, 2);
    drop(store);
    assert_eq!(decoded(root), at(Slot::A, 2));
    save_database(root, &scratch.path().join("at-2"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(root.join(watermark::NAME))
        .unwrap();
    file.write_all_at(&[0x5a], Slot::B.offset() + 100).unwrap();
    drop(file);
    assert_eq!(decoded(root), at(Slot::A, 2), "slot B is invalid");

    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    for (number, slot) in [(3u64, Slot::B), (4, Slot::A), (5, Slot::B)] {
        commits(&store, number, 1);
        assert_eq!(decoded(root), at(slot, number), "after commit {number}");
    }
    drop(store);
    restore_database(root, &scratch.path().join("at-2"));
    assert!(matches!(session(root), Err(Error::CommitLost)));
    assert!(matches!(
        reopen(root, Arc::new(Recorder::default())),
        Err(Error::CommitLost)
    ));
}

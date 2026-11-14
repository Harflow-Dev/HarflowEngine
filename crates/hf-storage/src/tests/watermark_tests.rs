#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Commit-watermark interleavings driven by stage hooks in a writer child: a writer that
//! dies between its slot write and its flush, readers at each stage of a publication, and
//! a binding lost after the commit.
mod support;
use super::super::{Error, WriterOptions};
use crate::write_tx::{Publish, Stage};
use hf_application::watermark::{Decoded, Slot};
use std::{fs, sync::Arc};
use support::{
    Recorder, Staged, database_seq, deadline, decoded, decoded_bytes, file_of, generation, open,
    put, root, session, tear,
};

const AFTER_COMMIT: &str = "watermark_test_after_commit";
const PAUSE: &str = "watermark_test_pause";

/// Child: `put` commits once and stops at `HF_WATERMARK_STAGE`; `binding` commits once,
/// stops after the commit, reports, stops again and commits a second time.
#[test]
fn watermark_stage_child() {
    let Ok(root) = std::env::var("HF_WATERMARK_ROOT") else {
        return;
    };
    let mode = std::env::var("HF_WATERMARK_MODE").unwrap();
    let store = open(root.as_ref(), WriterOptions::suppressed());
    if mode == "put" {
        crate::tests::select_stage(std::env::var("HF_WATERMARK_STAGE").unwrap());
        println!("RESULT {:?}", put(&store, "staged"));
        return;
    }
    crate::tests::select_stage(AFTER_COMMIT.into());
    let stage = Stage {
        before: "watermark_test_before_commit",
        after: AFTER_COMMIT,
    };
    let first = store.commit(deadline(), Publish::Nothing, Some(stage), |_| Ok(()));
    println!("RESULT first={first:?}");
    crate::tests::select_stage(PAUSE.into());
    crate::tests::stage(PAUSE);
    println!("RESULT second={:?}", put(&store, "second"));
}

fn valid(decoded: Decoded) -> u64 {
    match decoded {
        Decoded::Valid { seq, .. } => seq,
        other => panic!("no valid slot: {other:?}"),
    }
}

/// Writer 1 writes slot A (seq 2) and dies before its post-write flush. Writer 2 flushes
/// the file before it overwrites slot B, so a power loss at any later point keeps the
/// acknowledged value 1 or a value above it.
#[test]
fn a_writer_dying_before_its_flush_is_followed_by_a_pre_flush_of_its_slot() {
    let (_scratch, root) = root("dee-one", 1);
    let generation = generation(&root);
    let staged = Staged::start(&root, "put", "watermark_before_flush");
    staged.kill();
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::A,
            seq: 2
        }
    );
    assert_eq!(database_seq(&root), 2);

    let port = Arc::new(Recorder::default());
    let store = open(&root, WriterOptions::suppressed().with_flush(port.clone()));
    put(&store, "three").unwrap();
    drop(store);
    let images = port.images.lock().unwrap().clone();
    assert_eq!(images.len(), 2, "one pre-flush and one post-flush");
    let pre = file_of(&generation, Some(2), Some(1));
    assert_eq!(
        images[0], pre,
        "the pre-flush saw writer 1's slot before slot B changed"
    );
    let post = file_of(&generation, Some(2), Some(3));
    assert_eq!(images[1], post);

    // Power-loss model: writes after the last completed flush are lost; a torn slot write
    // leaves that slot invalid. `acknowledged` is the highest commit a caller saw succeed.
    let before = file_of(&generation, Some(0), Some(1));
    let mut torn_b = images[0].clone();
    torn_b[4096..4096 + 44].copy_from_slice(&images[1][4096..4096 + 44]);
    let durable_points = [(&before, 1), (&images[0], 1), (&torn_b, 1), (&images[1], 3)];
    for (index, (image, acknowledged)) in durable_points.into_iter().enumerate() {
        let seq = valid(decoded_bytes(image, &generation));
        assert!(seq >= acknowledged, "point {index}: {seq} < {acknowledged}");
    }
    // Without the pre-flush, writer 1's slot A could still be lost while slot B is torn:
    // neither the acknowledged value nor its replacement would remain.
    let mut unflushed = before.clone();
    unflushed[4096..4096 + 44].copy_from_slice(&torn_b[4096..4096 + 44]);
    let without = decoded_bytes(&unflushed, &generation);
    assert!(
        !matches!(without, Decoded::Valid { seq, .. } if seq >= 1),
        "{without:?}"
    );
    session(&root).unwrap();
}

#[test]
fn a_writer_dying_before_its_slot_write_leaves_a_readable_root_the_next_commit_catches_up() {
    let (_scratch, root) = root("before-write", 1);
    let staged = Staged::start(&root, "put", "watermark_before_write");
    staged.kill();
    assert_eq!(database_seq(&root), 2);
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::B,
            seq: 1
        }
    );
    let reader = session(&root).unwrap();
    reader.snapshot(deadline(), |_| Ok(())).unwrap();
    let store = open(&root, WriterOptions::suppressed());
    put(&store, "three").unwrap();
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::A,
            seq: 3
        }
    );
    reader.snapshot(deadline(), |_| Ok(())).unwrap();
}

/// After a torn slot, a reader at each stage of the next publications (before the slot
/// write, between write and flush) reads a valid watermark at or below its snapshot.
#[test]
fn readers_at_each_stage_after_a_torn_slot_never_see_an_unavailable_watermark() {
    let (_scratch, root) = root("coherent", 1);
    tear(&root, Slot::A);
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::B,
            seq: 1
        }
    );
    let early = session(&root).unwrap();
    let read_all = |label: &str| {
        session(&root)
            .and_then(|fresh| fresh.snapshot(deadline(), |_| Ok(())))
            .unwrap_or_else(|error| panic!("{label}: fresh session: {error:?}"));
        early
            .snapshot(deadline(), |_| Ok(()))
            .unwrap_or_else(|error| panic!("{label}: open session: {error:?}"));
    };
    let mut staged = Staged::start(&root, "put", "watermark_before_write");
    read_all("commit 2 before its slot write");
    staged.resume();
    assert_eq!(staged.expect("RESULT "), "Ok(())");
    staged.finish();
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::A,
            seq: 2
        }
    );

    tear(&root, Slot::B);
    let mut staged = Staged::start(&root, "put", "watermark_before_flush");
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::B,
            seq: 3
        }
    );
    read_all("commit 3 between its slot write and its flush");
    staged.resume();
    assert_eq!(staged.expect("RESULT "), "Ok(())");
    staged.finish();
    read_all("after both commits");
}

/// A binding check that fails after the commit is an unknown outcome, reported before
/// the slot write and without poisoning: the database is one ahead of the file, readers
/// keep working, and the next commit on the same handle publishes normally.
#[test]
fn a_binding_lost_after_the_commit_is_unknown_without_poisoning_the_handle() {
    let (_scratch, root) = root("binding", 1);
    let mut staged = Staged::start(&root, "binding", AFTER_COMMIT);
    let journal = root.join("store.sqlite3-journal");
    fs::create_dir(&journal).unwrap();
    staged.resume();
    assert_eq!(staged.expect("RESULT first="), "Err(CommitOutcomeUnknown)");
    staged.expect(&format!("STAGE:{PAUSE}"));
    // The directory also stops raw SQLite reads, so it goes before the database is read.
    fs::remove_dir(&journal).unwrap();
    assert_eq!(database_seq(&root), 2);
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::B,
            seq: 1
        }
    );
    session(&root)
        .unwrap()
        .snapshot(deadline(), |_| Ok(()))
        .unwrap();
    staged.resume();
    assert_eq!(staged.expect("RESULT second="), "Ok(())");
    staged.finish();
    assert_eq!(database_seq(&root), 3);
    assert_eq!(
        decoded(&root),
        Decoded::Valid {
            best: Slot::A,
            seq: 3
        }
    );
    assert!(!matches!(session(&root).err(), Some(Error::CommitLost)));
}

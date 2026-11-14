#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A database sequence below `store.watermark` is `CommitLost` at writer open, recover,
//! probe, read-session open and every read entry; the watermark is judged after the
//! fence, and a missing or slotless file is `ReadUnavailable("store.watermark")`.
mod common;
mod read_session_support;
mod verify_support;
mod watermark_support;
use hf_application::watermark::{NAME, Slot};
use hf_domain::task_read::AuthorizedScope;
use hf_storage::{Error, ReadRefusal};
use read_session_support::{later, open, page, read_all, seeded};
use std::{
    fs,
    os::unix::fs::symlink,
    time::{Duration, Instant},
};
use verify_support::{
    FENCE, VERIFY, assert_entries_refuse, fence_bytes, is_fenced, is_incompatible,
};
use watermark_support::{database_seq, decoded, file_bytes, file_of, generation, set_slot};

const ACCEPTANCE: &str = "store.wal-acceptance";

fn is_commit_lost(error: &Error) -> bool {
    matches!(error, Error::CommitLost)
}

fn is_unavailable(error: &Error) -> bool {
    matches!(error, Error::ReadUnavailable("store.watermark"))
}

/// The best slot moved `ahead` past the database sequence, in the non-best slot.
fn ahead_by(root: &std::path::Path, ahead: u64) {
    let seq = database_seq(root);
    set_slot(root, Slot::B, seq + ahead);
}

#[test]
fn every_entry_refuses_a_database_below_the_watermark() {
    let seeded = seeded(3);
    let root = &seeded.root;
    ahead_by(root, 1);
    assert_entries_refuse(root, is_commit_lost, &ReadRefusal::CommitLost, "one ahead");
    ahead_by(root, 1000);
    assert_entries_refuse(root, is_commit_lost, &ReadRefusal::CommitLost, "far ahead");
    // Refusing changed nothing: the watermark keeps what it said.
    assert_eq!(
        decoded(root),
        watermark_support::at(Slot::B, database_seq(root) + 1000)
    );
}

#[test]
fn a_watermark_at_or_below_the_database_is_readable_everywhere() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let seq = database_seq(root);
    assert!(seq > 0);
    for mark in [seq, seq - 1, 0] {
        fs::write(
            root.join(NAME),
            file_of(&generation(root), Some(mark), None),
        )
        .unwrap();
        let session = open(root).unwrap();
        assert_eq!(read_all(&session).unwrap().len(), 3, "mark {mark}");
        drop(session);
        verify_support::open_store(root).unwrap();
        hf_storage::probe(root, common::lease::lease_port()).unwrap();
        hf_storage::Store::recover(root, later(10), common::lease::lease_port()).unwrap();
    }
}

#[test]
fn an_open_session_refuses_each_read_entry_once_the_watermark_is_ahead() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let session = open(root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 3);
    let before = file_bytes(root);
    ahead_by(root, 1);
    let task = session.task_read(&AuthorizedScope::Global, &page(), later(5));
    assert!(matches!(task, Err(Error::CommitLost)), "{:?}", task.err());
    let snapshot = session.snapshot(later(5), |_| Ok(()));
    assert!(matches!(snapshot, Err(Error::CommitLost)), "{snapshot:?}");
    fs::write(root.join(NAME), &before).unwrap();
    assert_eq!(
        read_all(&session).unwrap().len(),
        3,
        "the refusal is not sticky"
    );
}

#[test]
fn the_fence_and_the_acceptance_record_are_judged_before_the_watermark() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let generation = generation(root);
    let good = file_bytes(root);
    fs::write(root.join(FENCE), fence_bytes(&generation, "failed")).unwrap();
    for (label, bytes) in [
        ("ahead", file_of(&generation, Some(0), Some(1000))),
        ("no valid slot", vec![0u8; 8192]),
        ("short", vec![0u8; 100]),
    ] {
        fs::write(root.join(NAME), &bytes).unwrap();
        assert_entries_refuse(root, is_fenced, &ReadRefusal::Fenced, label);
    }
    fs::remove_file(root.join(NAME)).unwrap();
    assert_entries_refuse(root, is_fenced, &ReadRefusal::Fenced, "missing");
    fs::remove_file(root.join(FENCE)).unwrap();

    let malformed = format!("harflow-wal-acceptance-1\n{generation}\ndone\n");
    fs::write(root.join(ACCEPTANCE), malformed).unwrap();
    fs::write(root.join(NAME), file_of(&generation, Some(0), Some(1000))).unwrap();
    assert_entries_refuse(
        root,
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "malformed acceptance record",
    );
    fs::remove_file(root.join(ACCEPTANCE)).unwrap();
    fs::write(root.join(NAME), &good).unwrap();
    assert_eq!(read_all(&open(root).unwrap()).unwrap().len(), 3);
}

#[test]
fn a_malformed_watermark_is_incompatible_even_when_a_slot_is_ahead() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let generation = generation(root);
    let foreign = "f".repeat(32);
    let ahead = file_of(&generation, Some(0), Some(1000));
    let mut foreign_slot = file_of(&foreign, Some(1), None);
    foreign_slot[4096..].copy_from_slice(&ahead[4096..]);
    let elsewhere = seeded.scratch.path().join("elsewhere");
    fs::write(&elsewhere, &ahead).unwrap();
    type Plant<'a> = (&'static str, Box<dyn Fn() + 'a>);
    let cases: Vec<Plant<'_>> = vec![
        (
            "foreign slot",
            Box::new(|| fs::write(root.join(NAME), &foreign_slot).unwrap()),
        ),
        (
            "one byte long",
            Box::new(|| fs::write(root.join(NAME), [ahead.as_slice(), &[0]].concat()).unwrap()),
        ),
        (
            "one byte short",
            Box::new(|| fs::write(root.join(NAME), &ahead[..8191]).unwrap()),
        ),
        (
            "symlink",
            Box::new(|| {
                fs::remove_file(root.join(NAME)).unwrap();
                symlink(&elsewhere, root.join(NAME)).unwrap();
            }),
        ),
        (
            "directory",
            Box::new(|| {
                fs::remove_file(root.join(NAME)).unwrap();
                fs::create_dir(root.join(NAME)).unwrap();
            }),
        ),
    ];
    for (label, plant) in cases {
        plant();
        assert_entries_refuse(root, is_incompatible, &ReadRefusal::InvalidMarker, label);
        let path = root.join(NAME);
        if path.is_dir() && !path.is_symlink() {
            fs::remove_dir(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        fs::write(&path, file_of(&generation, Some(0), None)).unwrap();
    }
}

#[test]
fn a_missing_or_slotless_watermark_is_unavailable_after_the_retries() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let refusal = ReadRefusal::Unavailable {
        file: "store.watermark",
    };
    let good = file_bytes(root);
    fs::remove_file(root.join(NAME)).unwrap();
    assert_entries_refuse(root, is_unavailable, &refusal, "missing");
    assert!(!root.join(NAME).exists(), "nothing recreates the file");

    let mut torn = good.clone();
    torn[10] ^= 1;
    torn[4096 + 10] ^= 1;
    fs::write(root.join(NAME), &torn).unwrap();
    assert_entries_refuse(root, is_unavailable, &refusal, "both slots torn");
    let started = Instant::now();
    assert!(open(root).is_err_and(|error| is_unavailable(&error)));
    assert!(
        started.elapsed() >= Duration::from_millis(3),
        "three retries 1 ms apart: {:?}",
        started.elapsed()
    );
    assert_eq!(
        fs::read(root.join(NAME)).unwrap(),
        torn,
        "nothing repairs the file"
    );
    fs::write(root.join(NAME), &good).unwrap();
    assert_eq!(read_all(&open(root).unwrap()).unwrap().len(), 3);
}

#[test]
fn verification_through_an_open_handle_refuses_a_database_below_the_watermark() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = verify_support::open_store(root).unwrap();
    ahead_by(root, 1);
    let due = store.verify_if_due(std::time::SystemTime::now(), later(30));
    assert!(matches!(due, Err(Error::CommitLost)), "{due:?}");
    let now = store.verify_in_place(later(30));
    assert!(matches!(now, Err(Error::CommitLost)), "{now:?}");
    assert!(!root.join(VERIFY).exists(), "no due record");
    assert!(!root.join(FENCE).exists(), "no fence");
}

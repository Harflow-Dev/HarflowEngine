#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The verifier records a verdict under `io.lock`, retrying a held lock until
//! `RECORD_BUDGET` runs out. A corruption verdict that cannot take the lock in time is
//! `FenceUnrecorded` and leaves no fence; a lock released within the budget still fences.
mod common;
mod read_session_support;
mod verify_support;
use hf_storage::{Error, RECORD_BUDGET, Verification};
use read_session_support::{later, seeded};
use std::{
    fs::File,
    path::Path,
    time::{Duration, Instant},
};
use verify_support::{FENCE, VERIFY, at, damaged, exists, open_store};

fn hold_io(root: &Path) -> File {
    let gate = File::open(root.join("io.lock")).unwrap();
    gate.try_lock().unwrap();
    gate
}

#[test]
fn a_lock_released_within_the_budget_still_fences() {
    let seeded = damaged();
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let gate = hold_io(root);
    let started = Instant::now();
    let outcome = std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(gate);
        });
        store.verify_in_place(later(30))
    });
    assert_eq!(outcome.unwrap(), Verification::Fenced);
    assert!(started.elapsed() < RECORD_BUDGET + Duration::from_secs(1));
    assert!(exists(root, FENCE));
}

#[test]
fn a_lock_held_past_the_budget_is_fence_unrecorded_and_leaves_no_fence() {
    let seeded = damaged();
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let gate = hold_io(root);
    let started = Instant::now();
    let outcome = store.verify_in_place(later(30));
    let took = started.elapsed();
    drop(gate);
    assert!(
        matches!(outcome, Err(Error::FenceUnrecorded(_))),
        "{outcome:?}"
    );
    assert!(
        took >= RECORD_BUDGET - Duration::from_millis(50),
        "{took:?}"
    );
    assert!(!exists(root, FENCE));
    assert!(store.verification_due(at(0)).unwrap());
}

#[test]
fn a_passing_scan_whose_record_lock_stays_held_records_nothing() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let gate = hold_io(root);
    let outcome = store.verify_in_place(later(30));
    drop(gate);
    assert!(
        matches!(outcome, Err(Error::Busy | Error::DeadlineExceeded)),
        "{outcome:?}"
    );
    assert!(!exists(root, VERIFY));
    assert!(!exists(root, FENCE));
}

#[test]
fn a_fence_write_that_fails_is_fence_unrecorded_with_its_cause() {
    let seeded = damaged();
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    // A directory at the temporary name makes the fence write fail with an I/O error.
    std::fs::create_dir(root.join("store.fence.tmp")).unwrap();
    let outcome = store.verify_in_place(later(30));
    match outcome {
        Err(Error::FenceUnrecorded(cause)) => {
            assert!(matches!(*cause, Error::Io(_)), "{cause:?}")
        }
        other => panic!("{other:?}"),
    }
    assert!(!exists(root, FENCE));
    assert!(store.verification_due(at(0)).unwrap());
}

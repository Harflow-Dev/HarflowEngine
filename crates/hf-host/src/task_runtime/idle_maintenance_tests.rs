#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{BASE, CAP, Cadence, poisons, step};
use crate::task_runtime::Error;
use hf_storage::{Error as StorageError, WalMaintenance};
use std::time::{Duration, Instant};

const COMPLETE: WalMaintenance = WalMaintenance::Checkpointed {
    complete: true,
    shrink_commit_landed: false,
};
const PINNED: WalMaintenance = WalMaintenance::Checkpointed {
    complete: false,
    shrink_commit_landed: false,
};

#[test]
fn idle_cadence_and_backoff_follow_outcomes() {
    let start = Instant::now();
    let mut cadence = Cadence::new();
    assert!(cadence.due(start), "the first call is due at once");

    cadence.settle(Some(&WalMaintenance::Idle), start);
    assert!(!cadence.due(start + BASE - Duration::from_millis(1)));
    assert!(cadence.due(start + BASE));

    let mut expected = BASE;
    for outcome in [WalMaintenance::Interrupted, WalMaintenance::Skipped, PINNED] {
        cadence.settle(Some(&outcome), start);
        expected = (expected * 2).min(CAP);
        assert_eq!(cadence.interval, expected, "{outcome:?}");
    }
    for _ in 0..10 {
        cadence.settle(Some(&WalMaintenance::Interrupted), start);
    }
    assert_eq!(cadence.interval, CAP, "the interval never passes the cap");
    assert!(!cadence.due(start + CAP - Duration::from_millis(1)));
    assert!(cadence.due(start + CAP));

    cadence.settle(None, start);
    assert_eq!(
        cadence.interval, CAP,
        "a failure that does not poison backs off"
    );

    cadence.settle(Some(&COMPLETE), start);
    assert_eq!(
        cadence.interval, BASE,
        "a complete checkpoint resets the pace"
    );
    cadence.settle(Some(&WalMaintenance::Interrupted), start);
    cadence.settle(Some(&WalMaintenance::Idle), start);
    assert_eq!(cadence.interval, BASE, "nothing due resets the pace");
}

#[test]
fn idle_error_classification_poisons_only_uncertain_outcomes() {
    for error in [
        StorageError::CommitOutcomeUnknown,
        StorageError::WatermarkUnrecorded,
        StorageError::CommitLost,
        StorageError::Replaced,
        StorageError::InvalidRoot,
    ] {
        assert!(poisons(&error), "{error:?}");
    }
    for error in [
        StorageError::Busy,
        StorageError::DeadlineExceeded,
        StorageError::Fenced,
        StorageError::WalMissing,
        StorageError::Incompatible,
        StorageError::Io(std::io::Error::other("noop read")),
    ] {
        assert!(!poisons(&error), "{error:?}");
    }
}

#[test]
fn idle_step_returns_poisoning_errors_to_the_loop() {
    let start = Instant::now();
    let mut cadence = Cadence::new();

    let mut calls = 0;
    let result = step(&mut cadence, start, || {
        calls += 1;
        Err(StorageError::CommitOutcomeUnknown)
    });
    assert!(matches!(
        result,
        Err(Error::Storage(StorageError::CommitOutcomeUnknown))
    ));
    assert_eq!(calls, 1);

    let mut cadence = Cadence::new();
    let result = step(&mut cadence, start, || Err(StorageError::WalMissing));
    assert!(result.is_ok(), "a non-poisoning failure is a skip");
    assert!(!cadence.due(start), "and it backs off");

    let mut cadence = Cadence::new();
    let mut called = false;
    cadence.settle(Some(&WalMaintenance::Idle), Instant::now());
    let result = step(&mut cadence, Instant::now(), || {
        called = true;
        Ok(WalMaintenance::Idle)
    });
    assert!(result.is_ok());
    assert!(!called, "a call that is not due is not made");
}

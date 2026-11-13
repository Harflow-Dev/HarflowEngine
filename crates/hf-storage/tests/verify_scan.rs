#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The verification scan is bounded by its deadline and never holds `io.lock`, so a
//! commit on the same handle proceeds while it runs.
mod common;
mod read_session_support;
mod verify_support;
use hf_storage::{Error, Mutation, Store, Verification};
use read_session_support::later;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use verify_support::{
    FENCE, VERIFY, at, due_bytes, generation, open_store, row_heavy, verification_files,
};

/// Rows enough for a scan of a few hundred milliseconds, at a few tens of MiB.
const ROWS: i64 = 600_000;

fn put(store: &Store, key: &str) -> hf_storage::Result<()> {
    store.apply_batch(
        &[Mutation::Put {
            key: key.into(),
            value: b"v".to_vec(),
        }],
        later(5),
    )
}

/// How long one unhurried check takes on `store`; it must pass.
fn full_scan(store: &Store) -> Duration {
    let started = Instant::now();
    assert_eq!(
        store.verify_in_place(later(120)).unwrap(),
        Verification::Passed
    );
    started.elapsed()
}

#[test]
fn a_deadline_inside_the_scan_is_inconclusive_and_writes_nothing() {
    let seeded = row_heavy(ROWS);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let full = full_scan(&store);
    assert!(full > Duration::from_millis(50), "scan too fast: {full:?}");
    // An expired due record that an inconclusive check must leave exactly as it is.
    let record = due_bytes(&generation(root), "1000");
    fs::write(root.join(VERIFY), &record).unwrap();
    let mut inconclusive = 0;
    for divisor in [10_u32, 6, 4, 3] {
        let started = Instant::now();
        let outcome = store.verify_if_due(at(5_000), Instant::now() + full / divisor);
        match outcome {
            Ok(Some(Verification::Inconclusive)) => inconclusive += 1,
            // Expired before the scan began: refused, equally without a record.
            Err(Error::DeadlineExceeded) => {}
            other => panic!("1/{divisor}: {other:?}"),
        }
        assert!(started.elapsed() < full, "1/{divisor}: not interrupted");
        assert_eq!(fs::read(root.join(VERIFY)).unwrap(), record, "1/{divisor}");
        assert_eq!(verification_files(root), vec![VERIFY.to_string()]);
    }
    assert!(inconclusive > 0, "no deadline landed inside the scan");
    assert!(!root.join(FENCE).exists());
    // Still due, and the handle is unharmed: it commits and the next check passes.
    assert!(store.verification_due(at(5_000)).unwrap());
    put(&store, "after").unwrap();
    assert_eq!(
        store.verify_if_due(at(5_000), later(120)).unwrap(),
        Some(Verification::Passed)
    );
}

#[test]
fn a_past_deadline_is_refused_before_any_scan() {
    let seeded = read_session_support::seeded(3);
    let store = open_store(&seeded.root).unwrap();
    let past = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        store.verify_in_place(past),
        Err(Error::DeadlineExceeded)
    ));
    assert!(matches!(
        store.verify_if_due(at(0), past),
        Err(Error::DeadlineExceeded)
    ));
    assert!(verification_files(&seeded.root).is_empty());
}

#[test]
fn a_commit_on_the_same_handle_is_not_blocked_by_a_running_scan() {
    let seeded = row_heavy(ROWS);
    let store = open_store(&seeded.root).unwrap();
    let full = full_scan(&store);
    let scanning = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let mut overlapped = 0;
    std::thread::scope(|scope| {
        let verifier = scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                scanning.store(true, Ordering::SeqCst);
                let outcome = store.verify_in_place(later(120));
                scanning.store(false, Ordering::SeqCst);
                assert_eq!(outcome.unwrap(), Verification::Passed);
            }
        });
        while !scanning.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        for number in 0..20 {
            let during = scanning.load(Ordering::SeqCst);
            let started = Instant::now();
            // A commit that collides with the verifier's brief record write may see
            // `Busy` from its own `io.lock` acquisition; it is retried, as a caller would.
            loop {
                match put(&store, &format!("commit-{number}")) {
                    Err(Error::Busy) => std::thread::yield_now(),
                    other => break other.unwrap(),
                }
            }
            // A commit that waited for the scan would take most of a full scan.
            let took = started.elapsed();
            assert!(
                took < full / 2 + Duration::from_millis(50),
                "{took:?} of {full:?}"
            );
            if during && scanning.load(Ordering::SeqCst) {
                overlapped += 1;
            }
        }
        stop.store(true, Ordering::SeqCst);
        verifier.join().unwrap();
    });
    assert!(overlapped > 0, "no commit overlapped a scan");
    assert!(!seeded.root.join(FENCE).exists());
}

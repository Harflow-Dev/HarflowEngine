#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The bounded scan: its deadline, its watchdog, and its independence from `io.lock`.
mod common;
mod in_place_support;
use common::{default_limits, durable_tree};
use hf_storage::{Error, Mutation, Store, probe, probe_with_deadline};
use in_place_support::{WAL, has_snapshot_dir, later, row_heavy_store, small_store};
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn put(store: &Store, key: &str) -> hf_storage::Result<()> {
    store.apply_batch(
        &[Mutation::Put {
            key: key.into(),
            value: b"v".to_vec(),
        }],
        later(5),
    )
}

#[test]
fn a_deadline_at_the_scan_start_still_interrupts_promptly() {
    let seeded = row_heavy_store();
    let root = &seeded.root;
    let started = Instant::now();
    probe_with_deadline(root, later(60), common::lease::lease_port())
        .unwrap()
        .unwrap();
    let full = started.elapsed();
    assert!(
        full > Duration::from_millis(100),
        "the store must scan long enough: {full:?}"
    );
    // Deadlines that expire around the moment the first scan step begins.
    for micros in [50_u64, 200, 1_000, 3_000, 10_000] {
        let started = Instant::now();
        let outcome = probe_with_deadline(
            root,
            Instant::now() + Duration::from_micros(micros),
            common::lease::lease_port(),
        );
        assert!(
            matches!(outcome, Err(Error::DeadlineExceeded)),
            "{micros}us: {outcome:?}"
        );
        assert!(started.elapsed() < full / 2, "{micros}us: not prompt");
    }
}

#[test]
fn a_deadline_that_expires_mid_scan_is_deadline_exceeded_for_probe_and_recover() {
    let seeded = row_heavy_store();
    let root = &seeded.root;
    let before = durable_tree(root);
    let started = Instant::now();
    probe_with_deadline(root, later(60), common::lease::lease_port())
        .unwrap()
        .unwrap();
    let full = started.elapsed();
    assert!(
        full > Duration::from_millis(100),
        "scan too fast to interrupt: {full:?}"
    );
    // A tenth of the measured scan: the prechecks fit, the scan does not.
    let started = Instant::now();
    let outcome = probe_with_deadline(
        root,
        Instant::now() + full / 10,
        common::lease::lease_port(),
    );
    assert!(
        matches!(outcome, Err(Error::DeadlineExceeded)),
        "{outcome:?}"
    );
    assert!(started.elapsed() < full, "the scan was not interrupted");
    let started = Instant::now();
    let outcome = Store::recover(
        root,
        Instant::now() + full / 10,
        common::lease::lease_port(),
    );
    assert!(
        matches!(outcome, Err(Error::DeadlineExceeded)),
        "{outcome:?}"
    );
    assert!(started.elapsed() < full, "the scan was not interrupted");
    assert!(durable_tree(root) == before);
    assert!(!has_snapshot_dir(root));
    // The same store still qualifies with room to finish.
    assert!(
        probe_with_deadline(root, later(60), common::lease::lease_port())
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_deadline_already_past_is_refused_before_any_scan() {
    let seeded = small_store();
    let past = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        probe_with_deadline(&seeded.root, past, common::lease::lease_port()),
        Err(Error::DeadlineExceeded)
    ));
    assert!(matches!(
        Store::recover(&seeded.root, past, common::lease::lease_port()),
        Err(Error::DeadlineExceeded)
    ));
}

#[test]
fn a_commit_is_not_blocked_while_a_probe_scans() {
    let seeded = row_heavy_store();
    let root = seeded.root.clone();
    let store = Store::open(
        &root,
        default_limits(),
        later(60),
        common::lease::lease_port(),
    )
    .unwrap();
    let scanning = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let mut overlapped = 0;
    std::thread::scope(|scope| {
        let prober = scope.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                scanning.store(true, Ordering::SeqCst);
                let outcome = probe_with_deadline(&root, later(60), common::lease::lease_port());
                scanning.store(false, Ordering::SeqCst);
                assert!(outcome.unwrap().is_some());
            }
        });
        while !scanning.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        for number in 0..20 {
            let during = scanning.load(Ordering::SeqCst);
            let started = Instant::now();
            put(&store, &format!("commit-{number}")).unwrap();
            // A commit that had to wait for a whole scan would take far longer.
            assert!(started.elapsed() < Duration::from_secs(2));
            if during && scanning.load(Ordering::SeqCst) {
                overlapped += 1;
            }
        }
        stop.store(true, Ordering::SeqCst);
        prober.join().unwrap();
    });
    assert!(overlapped > 0, "no commit overlapped a scan");
}

#[test]
fn an_io_gate_held_by_a_commit_does_not_stop_a_probe() {
    let seeded = small_store();
    let store = Store::open(
        &seeded.root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    put(&store, "before").unwrap();
    assert!(
        probe(&seeded.root, common::lease::lease_port())
            .unwrap()
            .is_some()
    );
    put(&store, "after").unwrap();
}

#[test]
fn a_quick_scan_ends_its_watchdog_and_leaves_the_next_operation_alone() {
    let seeded = small_store();
    let root = &seeded.root;
    // The deadline is far away: a watchdog that is not cancelled would hold the
    // probe until then; one that fires late would disturb what runs next.
    for round in 0..10 {
        let started = Instant::now();
        let identity = probe_with_deadline(root, later(30), common::lease::lease_port())
            .unwrap()
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10), "round {round}");
        let store = Store::open(
            root,
            default_limits(),
            later(30),
            common::lease::lease_port(),
        )
        .unwrap();
        assert_eq!(&identity, store.identity());
        put(&store, &format!("next-{round}")).unwrap();
        drop(store);
    }
    let started = Instant::now();
    Store::recover(root, later(30), common::lease::lease_port()).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_scan_finishing_just_before_its_deadline_is_not_interrupted_afterwards() {
    let seeded = small_store();
    let root = &seeded.root;
    let deadline = Instant::now() + Duration::from_millis(400);
    let outcome = probe_with_deadline(root, deadline, common::lease::lease_port());
    assert!(outcome.unwrap().is_some());
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(100),
    );
    let store = Store::open(
        root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    put(&store, "after-deadline").unwrap();
    assert!(probe(root, common::lease::lease_port()).unwrap().is_some());
}

#[test]
fn a_wal_swapped_during_the_scan_is_replaced() {
    let seeded = row_heavy_store();
    let root = seeded.root.clone();
    let wal = root.join(WAL);
    let copy = root.join("wal-copy");
    let mut replaced = false;
    for _ in 0..8 {
        fs::copy(&wal, &copy).unwrap();
        let outcome = std::thread::scope(|scope| {
            let prober =
                scope.spawn(|| probe_with_deadline(&root, later(60), common::lease::lease_port()));
            std::thread::sleep(Duration::from_millis(2));
            // Same bytes, new inode: the swap only shows in the identity check.
            let _ = fs::rename(&copy, &wal);
            prober.join().unwrap()
        });
        match outcome {
            Err(Error::Replaced) => {
                replaced = true;
                break;
            }
            Ok(_) | Err(Error::InvalidRoot | Error::WalMissing) => {}
            Err(other) => panic!("unexpected outcome {other:?}"),
        }
    }
    assert!(replaced, "no swap landed inside the scan window");
}

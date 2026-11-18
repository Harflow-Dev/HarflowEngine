#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Idle` maintenance state and the commit-path fallback gates, with the state backdated
//! instead of waiting for the 5 s window or the 60 s backoff.
use crate::{
    Limits, Mutation, Store, WalMaintenance, WriterOptions,
    tests::lease::{CountingLease, IoEvent},
    wal_idle::{ACK_WINDOW, Attempt, BACKOFF, INTERRUPT_LIMIT, Origin, State, hooks},
};
use hf_application::lease::LeasePort;
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(20);

#[test]
fn consecutive_interruptions_enter_long_backoff() {
    let start = Instant::now();
    let mut state = State::default();
    for _ in 1..INTERRUPT_LIMIT {
        state.begin(start);
        state.record(Attempt::Interrupted, start);
        assert!(state.allows(Origin::Tick, start));
        assert!(!state.exceeded());
    }
    state.begin(start);
    state.record(Attempt::Interrupted, start);
    assert!(state.exceeded());
    for origin in [Origin::Tick, Origin::Fallback] {
        assert!(!state.allows(origin, start + BACKOFF - Duration::from_millis(1)));
        assert!(state.allows(origin, start + BACKOFF + ACK_WINDOW));
    }
    assert!(state.exceeded(), "the record outlives the backoff itself");

    state.record(Attempt::Complete, start);
    assert!(!state.exceeded(), "a complete checkpoint resets the state");
    assert!(state.allows(Origin::Tick, start));
}

#[test]
fn an_incomplete_copy_breaks_the_run_of_interruptions() {
    let start = Instant::now();
    let mut state = State::default();
    for attempt in [
        Attempt::Interrupted,
        Attempt::Interrupted,
        Attempt::Incomplete,
        Attempt::Interrupted,
        Attempt::Interrupted,
    ] {
        state.record(attempt, start);
    }
    assert!(!state.exceeded());
    assert!(state.allows(Origin::Tick, start));
}

#[test]
fn the_fallback_waits_out_the_ack_window_but_the_tick_does_not() {
    let start = Instant::now();
    let mut state = State::default();
    assert!(state.allows(Origin::Fallback, start), "no attempt yet");
    state.begin(start);
    assert!(state.allows(Origin::Tick, start));
    assert!(!state.allows(
        Origin::Fallback,
        start + ACK_WINDOW - Duration::from_millis(1)
    ));
    assert!(state.allows(Origin::Fallback, start + ACK_WINDOW));
    state.record(Attempt::Complete, start);
    assert!(
        !state.allows(Origin::Fallback, start),
        "a complete copy keeps its start time for the window"
    );
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// A root with a 34 MiB uncheckpointed log, reopened with `owner_idle()`.
fn backlog(name: &str) -> (Scratch, PathBuf, Store) {
    let parent = std::env::temp_dir().join(format!("hf-storage-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = || crate::tests::lease::lease_port();
    drop(Store::initialize_fixture(&root, Limits::default(), deadline(), leases()).unwrap());
    let fill = Store::open(&root, Limits::default(), deadline(), leases()).unwrap();
    for number in 0..34u8 {
        put(&fill, number);
    }
    drop(fill);
    let options = WriterOptions::owner_idle();
    let store = Store::open_with(&root, Limits::default(), deadline(), leases(), options).unwrap();
    (scratch, root, store)
}

fn put(store: &Store, fill: u8) {
    let mutation = Mutation::Put {
        key: "bulk".into(),
        value: vec![fill; 1_000_000],
    };
    store.apply_batch(&[mutation], deadline()).unwrap();
}

fn main_bytes(root: &std::path::Path) -> Vec<u8> {
    fs::read(root.join("store.sqlite3")).unwrap()
}

#[test]
fn fallback_runs_no_copy_during_long_backoff() {
    let (_scratch, root, store) = backlog("fallback-backoff");
    let main = main_bytes(&root);
    {
        let mut state = store.idle.lock().unwrap();
        state.set_last_attempt(Some(2 * ACK_WINDOW));
        state.set_backoff(Some(Instant::now() + BACKOFF));
    }
    put(&store, 50);
    assert_eq!(
        main_bytes(&root),
        main,
        "the long backoff gates the fallback"
    );
    store.idle.lock().unwrap().set_backoff(None);
    put(&store, 51);
    assert_ne!(
        main_bytes(&root),
        main,
        "control: without the backoff it copies"
    );
}

#[test]
fn fallback_is_skipped_when_a_maintenance_attempt_started_within_the_ack_window() {
    let (_scratch, root, store) = backlog("fallback-window");
    let main = main_bytes(&root);
    store
        .idle
        .lock()
        .unwrap()
        .set_last_attempt(Some(Duration::ZERO));
    put(&store, 50);
    assert_eq!(
        main_bytes(&root),
        main,
        "an attempt in the window gates the fallback"
    );
    store
        .idle
        .lock()
        .unwrap()
        .set_last_attempt(Some(2 * ACK_WINDOW));
    put(&store, 51);
    assert_ne!(
        main_bytes(&root),
        main,
        "control: outside the window it copies"
    );
}

#[test]
fn an_interruption_at_a_shortened_budget_does_not_count_toward_the_backoff() {
    let start = Instant::now();
    let mut state = State::default();
    for _ in 0..2 * INTERRUPT_LIMIT {
        state.record(Attempt::Cut, start);
    }
    assert!(!state.exceeded());
    assert!(state.allows(Origin::Tick, start));
    state.record(Attempt::Interrupted, start);
    state.record(Attempt::Interrupted, start);
    state.record(Attempt::Cut, start);
    state.record(Attempt::Interrupted, start);
    assert!(state.exceeded(), "a cut neither counts nor breaks the run");
}

fn scratch_root(name: &str) -> (Scratch, PathBuf) {
    let parent = std::env::temp_dir().join(format!("hf-storage-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = crate::tests::lease::lease_port();
    drop(Store::initialize_fixture(&root, Limits::default(), deadline(), leases).unwrap());
    (scratch, root)
}

/// `batches` commits of 64 distinct 1 MB values through a Suppressed handle.
fn distinct_fill(root: &Path, batches: u8) {
    let limits = Limits {
        max_rows: 128,
        max_bytes: 70_000_000,
    };
    let leases = crate::tests::lease::lease_port();
    let store = Store::open_with(
        root,
        limits,
        deadline(),
        leases,
        WriterOptions::suppressed(),
    )
    .unwrap();
    for batch in 0..batches {
        let mutations: Vec<_> = (0..64u8)
            .map(|row| Mutation::Put {
                key: format!("distinct-{batch}-{row}"),
                value: vec![row; 1_000_000],
            })
            .collect();
        store.apply_batch(&mutations, deadline()).unwrap();
    }
}

fn idle(root: &Path, budget: Duration, lease: Arc<dyn LeasePort>) -> Store {
    let mut options = WriterOptions::owner_idle();
    options.passive_budget = budget;
    Store::open_with(root, Limits::default(), deadline(), lease, options).unwrap()
}

fn noop(root: &Path) -> (i64, i64) {
    crate::tests::raw_connection(root.join("store.sqlite3"))
        .query_row("PRAGMA wal_checkpoint(NOOP)", [], |row| {
            Ok((row.get(1)?, row.get(2)?))
        })
        .unwrap()
}

#[test]
fn a_deadline_that_passes_before_the_statement_starts_is_not_an_interruption() {
    let (_scratch, root) = scratch_root("not-entered");
    let connection = crate::tests::raw_connection(root.join("store.sqlite3"));
    let entered = Cell::new(false);
    let result = crate::wal_idle::passive_for_test(&connection, Instant::now(), || {
        entered.set(true);
    });
    assert!(
        matches!(result, Err(crate::Error::DeadlineExceeded)),
        "{result:?}"
    );
    assert!(!entered.get(), "the statement never started");
}

/// SQLite exposes no synchronization point inside the checkpoint copy loop, so the interrupt
/// timing here is margin-based: the workload's full copy is assumed to take at least ten
/// times the budget. The property that an interrupted PASSIVE publishes no progress was
/// measured separately (plan D4 notes).
#[test]
fn owner_idle_passive_respects_scan_watchdog() {
    let budget = Duration::from_millis(50);
    let (_scratch, root) = scratch_root("watchdog");
    distinct_fill(&root, 12);
    let (frames, copied) = noop(&root);
    assert!(frames > 170_000 && copied == 0, "{frames} {copied}");

    let store = idle(&root, budget, crate::tests::lease::lease_port());
    let before = hooks::entries();
    let outcome = store.maintain_wal(deadline()).unwrap();
    assert!(
        matches!(outcome, WalMaintenance::Interrupted),
        "{outcome:?}: the copy of this workload finished inside {budget:?}; the test assumes \
         a full copy takes at least ten times the budget, so enlarge the workload"
    );
    assert!(
        hooks::entries() > before,
        "the checkpoint statement had started"
    );
    assert_eq!(noop(&root), (frames, 0), "no progress was published");
    assert!(!store.wal_envelope_exceeded());
    drop(store);

    // Control on the same workload with a test-only budget that bypasses the production cap.
    let connection = crate::tests::raw_connection(root.join("store.sqlite3"));
    let begun = Instant::now();
    let done = crate::wal_idle::passive_for_test(
        &connection,
        Instant::now() + Duration::from_secs(10),
        || {},
    )
    .unwrap()
    .expect("the long budget completes");
    assert!(done.complete, "{done:?}");
    assert!(
        begun.elapsed() >= 10 * budget,
        "the copy took {:?}: shorter than ten times the budget",
        begun.elapsed()
    );
}

#[test]
fn controlled_contention_two_idle_owners_and_a_sibling_commit() {
    let (_scratch, root) = scratch_root("contention");
    distinct_fill(&root, 2);
    let counted = || CountingLease::new();
    let (first_lease, rival_lease, sibling_lease) = (counted(), counted(), counted());
    let port = |lease: &Arc<CountingLease>| -> Arc<dyn LeasePort> { lease.clone() };
    let first = idle(&root, Duration::from_millis(1500), port(&first_lease));
    let rival = idle(&root, Duration::from_millis(50), port(&rival_lease));
    let sibling = idle(&root, Duration::from_millis(1500), port(&sibling_lease));
    let (rival_events, sibling_events) = (rival_lease.events(), sibling_lease.events());
    let (held_tx, held_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let copy = scope.spawn(|| {
            hooks::set_after_io_lock(Some(Box::new(move || {
                held_tx.send(()).unwrap();
                go_rx.recv_timeout(WAIT).unwrap();
            })));
            let result = first.maintain_wal(Instant::now() + WAIT);
            hooks::set_after_io_lock(None);
            result
        });
        // The first attempt is parked holding `io.lock`.
        held_rx.recv_timeout(WAIT).unwrap();
        let contender = scope.spawn(|| rival.maintain_wal(Instant::now() + Duration::from_secs(5)));
        let commit = scope.spawn(|| {
            let put = Mutation::Put {
                key: "sibling".into(),
                value: vec![7; 32],
            };
            sibling.apply_batch(&[put], Instant::now() + WAIT)
        });
        for events in [&rival_events, &sibling_events] {
            assert_eq!(events.recv_timeout(WAIT), Ok(IoEvent::Busy));
        }
        go_tx.send(()).unwrap();
        commit.join().unwrap().unwrap();
        let copied = copy.join().unwrap().unwrap();
        assert!(
            matches!(
                copied,
                WalMaintenance::Checkpointed { .. } | WalMaintenance::Interrupted
            ),
            "{copied:?}"
        );
        let contended = contender.join().unwrap().unwrap();
        assert!(
            matches!(
                contended,
                WalMaintenance::Idle
                    | WalMaintenance::Skipped
                    | WalMaintenance::Interrupted
                    | WalMaintenance::Checkpointed { .. }
            ),
            "{contended:?}"
        );
    });
    assert!(!root.join("store.fence").exists());
    let read = sibling.read_batch(&["sibling".into()], deadline()).unwrap();
    assert_eq!(read[0].value, vec![7; 32]);
}

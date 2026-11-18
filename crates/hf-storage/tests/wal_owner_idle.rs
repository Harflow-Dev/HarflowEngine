#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `owner_idle()` handles: maintenance belongs to the holder's idle loop, the copy runs on a
//! transient connection under the watchdog, and the commit path falls back only above the
//! hard live-log bound. Backlogs are filled through a Suppressed handle where a test needs
//! the log to grow without maintenance.
mod common;
mod wal_owner_support;
use common::{Fixture, deadline};
use hf_storage::{JOURNAL_SIZE_LIMIT, PASSIVE_AFTER, WalMaintenance, WriterOptions};
use std::time::Duration;
use wal_owner_support::{
    BULK, DONE, MAIN, ONE_COMMIT, SHRUNK, bulk, bytes, distinct_fill, idle_with, initialized, noop,
    open, owner_idle, pin, put, release, suppressed_fill, value, wal_len,
};

const SHORT: Duration = Duration::from_millis(10);

#[test]
fn owner_idle_commits_below_hard_bound_leave_maintenance_to_the_holder() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner_idle(&root);
    let main = bytes(&root, MAIN);
    bulk(&store, 30);
    assert!(wal_len(&root) > PASSIVE_AFTER, "past the legacy trigger");
    assert_eq!(bytes(&root, MAIN), main, "no commit-path checkpoint");
    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_ne!(bytes(&root, MAIN), main, "the tick copied the backlog");
    assert_eq!(wal_len(&root), JOURNAL_SIZE_LIMIT);
    assert_eq!(value(&store, "bulk"), vec![29; BULK]);
}

#[test]
fn owner_idle_loop_checkpoints_wal_below_bound() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner_idle(&root);
    let (mut largest, mut checkpoints) = (0, 0);
    for fill in 0..40u8 {
        put(&store, "bulk", BULK, fill);
        if matches!(
            store.maintain_wal(deadline()).unwrap(),
            WalMaintenance::Checkpointed { complete: true, .. }
        ) {
            checkpoints += 1;
        }
        let length = wal_len(&root);
        assert!(
            length <= PASSIVE_AFTER + 2 * ONE_COMMIT,
            "tick {fill}: {length}"
        );
        largest = largest.max(length);
    }
    assert!(
        largest > PASSIVE_AFTER && checkpoints >= 3,
        "{largest} {checkpoints}"
    );
    assert_eq!(value(&store, "bulk"), vec![39; BULK]);
}

#[test]
fn owner_commit_path_fallback_above_hard_bound() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let store = owner_idle(&root);
    let main = bytes(&root, MAIN);
    let mut fell_back = None;
    for fill in 0..40u8 {
        put(&store, "bulk", BULK, fill);
        if bytes(&root, MAIN) != main {
            fell_back = Some(fill);
            break;
        }
    }
    let fill = fell_back.expect("the commit path must copy once the log passes the hard bound");
    assert!(fill >= 30, "not before the hard bound: commit {fill}");
    assert_eq!(value(&store, "bulk"), vec![fill; BULK]);
    store.maintain_wal(deadline()).unwrap();
    assert!(wal_len(&root) <= JOURNAL_SIZE_LIMIT + ONE_COMMIT);
}

#[test]
fn owner_idle_passive_completes_within_the_default_budget() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    suppressed_fill(&root, 34);
    let store = owner_idle(&root);
    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_eq!(value(&store, "bulk"), vec![33; BULK]);
    assert!(!store.wal_envelope_exceeded());
}

#[test]
fn owner_idle_open_performs_no_checkpoint() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    suppressed_fill(&root, 10);
    let main = bytes(&root, MAIN);
    let idle = owner_idle(&root);
    assert_eq!(bytes(&root, MAIN), main, "an Idle open validates only");
    drop(idle);
    let legacy = open(&root, WriterOptions::owner());
    assert_ne!(
        bytes(&root, MAIN),
        main,
        "owner() keeps its open-time PASSIVE"
    );
    drop(legacy);
}

#[test]
fn fully_checkpointed_log_is_not_recopied_until_it_restarts() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    suppressed_fill(&root, 34);
    let store = owner_idle(&root);
    let reader = pin(&root);
    // The reader holds the whole log: the copy completes, the log cannot restart.
    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    let main = bytes(&root, MAIN);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    put(&store, "bulk", BULK, 99);
    put(&store, "bulk", BULK, 100);
    assert_eq!(
        bytes(&root, MAIN),
        main,
        "no commit-path copy of a drained log"
    );
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    release(reader);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    assert_eq!(bytes(&root, MAIN), main);
    assert_eq!(value(&store, "bulk"), vec![100; BULK]);
}

#[test]
fn backlog_inside_envelope_completes_after_earlier_interruptions() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    distinct_fill(&root, 1);
    let store = idle_with(&root, SHORT);
    for _ in 0..2 {
        assert_eq!(
            store.maintain_wal(deadline()).unwrap(),
            WalMaintenance::Interrupted
        );
    }
    assert!(!store.wal_envelope_exceeded());
    let (frames, copied) = noop(&root);
    assert!(frames > 0 && copied == 0, "no progress was published");
    let result = store
        .maintain_wal_with(deadline(), Duration::from_millis(1500))
        .unwrap();
    assert!(
        matches!(result, WalMaintenance::Checkpointed { complete: true, .. }),
        "{result:?}"
    );
    assert!(
        !store.wal_envelope_exceeded(),
        "a complete copy resets the state"
    );
    assert_eq!(value(&store, "distinct-0-63")[..4], [63u8; 4]);
}

#[test]
fn backlog_beyond_envelope_enters_long_backoff_and_reports() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    distinct_fill(&root, 2);
    let store = idle_with(&root, SHORT);
    for _ in 0..3 {
        assert_eq!(
            store.maintain_wal(deadline()).unwrap(),
            WalMaintenance::Interrupted
        );
    }
    assert!(store.wal_envelope_exceeded(), "the condition is recorded");
    let main = bytes(&root, MAIN);
    assert_eq!(
        store
            .maintain_wal_with(deadline(), Duration::from_millis(1500))
            .unwrap(),
        WalMaintenance::Skipped,
        "idle maintenance pauses for the backoff"
    );
    assert_eq!(bytes(&root, MAIN), main);
    assert_eq!(value(&store, "distinct-1-63")[..4], [63u8; 4]);
}

#[test]
fn idle_owner_beside_suppressed_writer_and_reader_keeps_data_and_reports_truthfully() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let idle = owner_idle(&root);
    let sibling = open(&root, WriterOptions::suppressed());
    let reader = pin(&root);
    bulk(&sibling, 18);
    assert_eq!(
        idle.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Checkpointed {
            complete: false,
            shrink_commit_landed: false
        },
        "a reader pinning the log is reported as an incomplete copy"
    );
    assert_eq!(value(&idle, "bulk"), vec![17; BULK]);
    release(reader);
    assert_eq!(idle.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_eq!(wal_len(&root), JOURNAL_SIZE_LIMIT);
    // The sibling restarts the log without cutting the file; nothing is due.
    put(&sibling, "later", 64, 5);
    assert_eq!(idle.maintain_wal(deadline()).unwrap(), WalMaintenance::Idle);
    assert_eq!(value(&idle, "later"), vec![5; 64]);
    assert_eq!(value(&sibling, "bulk"), vec![17; BULK]);
}

#[test]
fn skipped_shrink_is_retried_by_a_later_idle_call_without_new_writes() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    suppressed_fill(&root, 18);
    let store = owner_idle(&root);
    let grown = wal_len(&root);
    assert!(grown > JOURNAL_SIZE_LIMIT, "{grown}");
    let writer = common::raw_connection(root.join(MAIN));
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        DONE,
        "the copy completed, the shrink commit was skipped"
    );
    assert_eq!(wal_len(&root), grown);
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    let (frames, copied) = noop(&root);
    assert_eq!(frames, copied, "nothing left to copy");
    assert_eq!(store.maintain_wal(deadline()).unwrap(), SHRUNK);
    assert_eq!(wal_len(&root), JOURNAL_SIZE_LIMIT);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    assert_eq!(value(&store, "bulk"), vec![17; BULK]);
}

#[test]
fn an_interruption_at_a_shortened_budget_does_not_enter_the_backoff() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    distinct_fill(&root, 2);
    let store = owner_idle(&root);
    for _ in 0..4 {
        assert_eq!(
            store.maintain_wal_with(deadline(), SHORT).unwrap(),
            WalMaintenance::Interrupted
        );
    }
    assert!(
        !store.wal_envelope_exceeded(),
        "short budgets say nothing about the envelope"
    );
}

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The lost-WAL acceptance record: which of its three states lets a root be used, at
//! every entry and at each later `read()`, and that no reader ever writes it.
mod common;
mod read_session_support;
mod wal_acceptance_support;
use common::durable_tree;
use hf_storage::{Error, ReadRefusal, probe};
use read_session_support::{WAL, checkpoint, open, read_all, seeded};
use std::fs;
use wal_acceptance_support::{ENTRIES, RECORD, assert_refused, clear, is_wal_missing, put_state};

#[test]
fn an_absent_or_accepted_record_lets_every_entry_proceed() {
    for state in [None, Some("accepted")] {
        for (name, entry) in ENTRIES {
            let seeded = seeded(3);
            if let Some(state) = state {
                put_state(&seeded.root, state);
            }
            entry(&seeded.root).unwrap_or_else(|error| panic!("{name}, {state:?}: {error:?}"));
        }
    }
}

#[test]
fn a_pending_or_failed_record_is_a_missing_wal_at_every_entry() {
    for state in ["pending", "failed"] {
        let seeded = seeded(3);
        put_state(&seeded.root, state);
        for (name, entry) in ENTRIES {
            assert_refused(
                entry(&seeded.root),
                is_wal_missing,
                &ReadRefusal::WalMissing,
                &format!("{name}, {state}"),
            );
        }
    }
}

#[test]
fn an_empty_wal_does_not_change_the_outcome_for_any_state() {
    for (state, proceeds) in [("accepted", true), ("pending", false), ("failed", false)] {
        for (name, entry) in ENTRIES {
            let seeded = seeded(3);
            checkpoint(&seeded.root);
            assert_eq!(fs::metadata(seeded.root.join(WAL)).unwrap().len(), 0);
            put_state(&seeded.root, state);
            let outcome = entry(&seeded.root);
            if proceeds {
                outcome.unwrap_or_else(|error| panic!("{name}, {state}: {error:?}"));
            } else {
                assert_refused(
                    outcome,
                    is_wal_missing,
                    &ReadRefusal::WalMissing,
                    &format!("{name}, {state}, empty wal"),
                );
            }
        }
    }
}

#[test]
fn a_record_created_after_open_refuses_the_next_read_and_recovers_when_removed() {
    for state in ["pending", "failed"] {
        let seeded = seeded(3);
        let session = open(&seeded.root).unwrap();
        assert_eq!(read_all(&session).unwrap().len(), 3);
        put_state(&seeded.root, state);
        for _ in 0..2 {
            assert_refused(
                read_all(&session).map(drop),
                is_wal_missing,
                &ReadRefusal::WalMissing,
                state,
            );
        }
        clear(&seeded.root);
        assert_eq!(read_all(&session).unwrap().len(), 3, "{state}: removed");
    }
}

#[test]
fn a_record_changed_after_open_is_honoured_by_the_next_read() {
    let seeded = seeded(3);
    put_state(&seeded.root, "accepted");
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 3);
    for state in ["pending", "accepted", "failed", "accepted"] {
        put_state(&seeded.root, state);
        let outcome = read_all(&session);
        if state == "accepted" {
            assert_eq!(outcome.unwrap().len(), 3, "{state}");
        } else {
            assert!(matches!(outcome, Err(Error::WalMissing)), "{state}");
        }
    }
}

#[test]
fn readers_create_and_modify_no_file_in_any_state() {
    for state in [None, Some("accepted"), Some("pending"), Some("failed")] {
        let seeded = seeded(3);
        if let Some(state) = state {
            put_state(&seeded.root, state);
        }
        let before = durable_tree(&seeded.root);
        let record_before = fs::read(seeded.root.join(RECORD)).ok();
        let _ = probe(&seeded.root, common::lease::lease_port());
        if let Ok(session) = open(&seeded.root) {
            let _ = read_all(&session);
        }
        assert!(durable_tree(&seeded.root) == before, "{state:?}");
        assert_eq!(fs::read(seeded.root.join(RECORD)).ok(), record_before);
        assert_eq!(seeded.root.join(RECORD).exists(), state.is_some());
    }
}

#[test]
fn refused_open_and_recover_create_and_modify_no_file() {
    for state in ["pending", "failed"] {
        let seeded = seeded(3);
        put_state(&seeded.root, state);
        let before = durable_tree(&seeded.root);
        for (name, entry) in &ENTRIES[..] {
            assert!(entry(&seeded.root).is_err(), "{name}");
            assert!(durable_tree(&seeded.root) == before, "{name}, {state}");
        }
    }
}

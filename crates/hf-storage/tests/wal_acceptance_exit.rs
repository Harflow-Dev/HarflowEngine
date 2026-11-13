#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The acceptance record is checked again when a read transaction ends: a record that
//! changes while the read runs refuses that read, not only the next one.
mod common;
mod read_session_support;
mod wal_acceptance_support;
use hf_storage::{Error, ReadRefusal, ReadSession};
use read_session_support::{later, open, read_all, seeded};
use std::path::Path;
use wal_acceptance_support::{
    assert_refused, clear, generation, is_incompatible, is_wal_missing, put_bytes, put_state,
    record_bytes,
};

/// One read whose action plants a record half way through the transaction.
fn read_planting(
    session: &ReadSession,
    root: &Path,
    plant: impl FnOnce(&Path),
) -> hf_storage::Result<()> {
    session.snapshot(later(5), |_| {
        plant(root);
        Ok(())
    })
}

#[test]
fn a_pending_or_failed_record_planted_during_a_read_refuses_that_read() {
    for state in ["pending", "failed"] {
        let seeded = seeded(3);
        let session = open(&seeded.root).unwrap();
        assert_refused(
            read_planting(&session, &seeded.root, |root| put_state(root, state)),
            is_wal_missing,
            &ReadRefusal::WalMissing,
            &format!("{state} during read"),
        );
        clear(&seeded.root);
        assert_eq!(read_all(&session).unwrap().len(), 3, "{state}: cleared");
    }
}

#[test]
fn a_foreign_or_malformed_record_planted_during_a_read_is_incompatible() {
    let seeded = seeded(3);
    let other = "f".repeat(32);
    assert_ne!(other, generation(&seeded.root));
    let cases: [(&str, Vec<u8>); 2] = [
        ("foreign accepted", record_bytes(&other, "accepted")),
        ("malformed", b"not a record\n".to_vec()),
    ];
    let session = open(&seeded.root).unwrap();
    for (label, bytes) in cases {
        assert_refused(
            read_planting(&session, &seeded.root, |root| put_bytes(root, &bytes)),
            is_incompatible,
            &ReadRefusal::InvalidMarker,
            label,
        );
        clear(&seeded.root);
        assert_eq!(read_all(&session).unwrap().len(), 3, "{label}: cleared");
    }
}

#[test]
fn a_record_accepted_or_removed_during_a_read_lets_it_complete() {
    let seeded = seeded(3);
    let session = open(&seeded.root).unwrap();
    read_planting(&session, &seeded.root, |root| put_state(root, "accepted")).unwrap();
    read_planting(&session, &seeded.root, clear).unwrap();
    assert!(!seeded.root.join(wal_acceptance_support::RECORD).exists());
    assert!(matches!(read_all(&session), Ok(rows) if rows.len() == 3));
}

#[test]
fn a_refused_exit_check_does_not_poison_the_session() {
    let seeded = seeded(3);
    let session = open(&seeded.root).unwrap();
    let outcome = read_planting(&session, &seeded.root, |root| put_state(root, "pending"));
    assert!(matches!(outcome, Err(Error::WalMissing)), "{outcome:?}");
    assert!(matches!(read_all(&session), Err(Error::WalMissing)));
    put_state(&seeded.root, "accepted");
    assert_eq!(read_all(&session).unwrap().len(), 3);
}

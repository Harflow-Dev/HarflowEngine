#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Refusal order around the acceptance record: marker, header, rollback journal and
//! `-wal` come first; the record comes before the later identity checks.
mod common;
mod read_session_support;
mod wal_acceptance_support;
use hf_storage::{Error, ReadRefusal, SCHEMA_VERSION};
use read_session_support::{MAIN, Seeded, WAL, checkpoint, edit_main, open, read_all, seeded};
use std::fs;
use wal_acceptance_support::{
    ENTRIES, assert_refused, is_wal_missing, put_bytes, put_state, record_bytes,
};

fn any_error(_: &Error) -> bool {
    true
}

/// With a pending record already in place, every entry must report `refusal`, not the
/// record's own `WalMissing`.
fn assert_beats_pending_record(seeded: &Seeded, refusal: &ReadRefusal, label: &str) {
    for (name, entry) in ENTRIES {
        assert_refused(
            entry(&seeded.root),
            any_error,
            refusal,
            &format!("{label}: {name}"),
        );
    }
}

#[test]
fn a_missing_or_non_regular_wal_beats_a_malformed_record() {
    for kind in ["missing", "directory", "symlink"] {
        let seeded = seeded(3);
        let wal = seeded.root.join(WAL);
        fs::remove_file(&wal).unwrap();
        let _ = fs::remove_file(seeded.root.join("store.sqlite3-shm"));
        match kind {
            "directory" => fs::create_dir(&wal).unwrap(),
            "symlink" => {
                let target = seeded.scratch.path().join("elsewhere-wal");
                fs::write(&target, b"").unwrap();
                std::os::unix::fs::symlink(&target, &wal).unwrap();
            }
            _ => {}
        }
        // Reached, this record would be Incompatible.
        put_bytes(&seeded.root, &record_bytes(&"f".repeat(32), "accepted"));
        for (name, entry) in ENTRIES {
            assert_refused(
                entry(&seeded.root),
                is_wal_missing,
                &ReadRefusal::WalMissing,
                &format!("{kind}: {name}"),
            );
        }
    }
}

#[test]
fn a_wal_that_goes_bad_after_open_beats_a_malformed_record_on_read() {
    let seeded = seeded(3);
    let session = open(&seeded.root).unwrap();
    put_bytes(&seeded.root, &record_bytes(&"f".repeat(32), "accepted"));
    fs::remove_file(seeded.root.join(WAL)).unwrap();
    assert_refused(
        read_all(&session).map(drop),
        is_wal_missing,
        &ReadRefusal::WalMissing,
        "read",
    );
}

#[test]
fn a_bad_marker_beats_the_record() {
    let seeded = seeded(3);
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let generation = marker.lines().nth(2).unwrap();
    put_state(&seeded.root, "pending");
    fs::write(
        seeded.root.join("store.identity"),
        format!("harflow-storage-2\n{}\n{generation}\n", SCHEMA_VERSION + 1),
    )
    .unwrap();
    assert_beats_pending_record(&seeded, &ReadRefusal::FutureSchema, "newer marker");
    fs::write(seeded.root.join("store.identity"), "garbage").unwrap();
    assert_beats_pending_record(&seeded, &ReadRefusal::InvalidMarker, "garbled marker");
}

#[test]
fn a_bad_header_beats_the_record() {
    let seeded = seeded(3);
    checkpoint(&seeded.root);
    put_state(&seeded.root, "pending");
    let mut main = fs::read(seeded.root.join(MAIN)).unwrap();
    main[68..72].copy_from_slice(&1u32.to_be_bytes());
    fs::write(seeded.root.join(MAIN), main).unwrap();
    // The writer-side header check reports a foreign application id as an invalid
    // marker; the read session names it.
    for (name, entry) in ENTRIES {
        let want = if name == "read session" {
            ReadRefusal::ForeignStore
        } else {
            ReadRefusal::InvalidMarker
        };
        assert_refused(entry(&seeded.root), any_error, &want, name);
    }
}

#[test]
fn a_nonempty_journal_beats_the_record() {
    let seeded = seeded(3);
    put_state(&seeded.root, "pending");
    fs::write(seeded.root.join("store.sqlite3-journal"), b"pending").unwrap();
    assert_beats_pending_record(&seeded, &ReadRefusal::NonemptyJournal, "journal");
}

#[test]
fn the_record_beats_a_changed_sql_identity_at_session_open() {
    let seeded = seeded(3);
    edit_main(
        &seeded.root,
        "UPDATE store_identity SET generation=lower(hex(randomblob(16)));",
    );
    assert_refused(
        open(&seeded.root).map(drop),
        |error| matches!(error, Error::Incompatible),
        &ReadRefusal::InvalidMarker,
        "control",
    );
    put_state(&seeded.root, "pending");
    assert_refused(
        open(&seeded.root).map(drop),
        is_wal_missing,
        &ReadRefusal::WalMissing,
        "pending",
    );
}

#[test]
fn the_record_beats_a_changed_sql_identity_on_read() {
    let seeded = seeded(3);
    let session = open(&seeded.root).unwrap();
    let other = rusqlite::Connection::open(seeded.root.join(MAIN)).unwrap();
    other
        .execute(
            "UPDATE store_identity SET generation=lower(hex(randomblob(16)))",
            [],
        )
        .unwrap();
    assert_refused(
        read_all(&session).map(drop),
        |error| matches!(error, Error::Replaced),
        &ReadRefusal::Replaced,
        "control",
    );
    put_state(&seeded.root, "failed");
    assert_refused(
        read_all(&session).map(drop),
        is_wal_missing,
        &ReadRefusal::WalMissing,
        "failed",
    );
    drop(other);
}

#[test]
fn a_changed_marker_after_open_is_replaced_before_the_record_is_consulted() {
    let seeded = seeded(3);
    let session = open(&seeded.root).unwrap();
    let marker = format!("harflow-storage-2\n{SCHEMA_VERSION}\n{}\n", "e".repeat(32));
    fs::write(seeded.root.join("store.identity"), marker).unwrap();
    put_state(&seeded.root, "pending");
    assert_refused(
        read_all(&session).map(drop),
        |error| matches!(error, Error::Replaced),
        &ReadRefusal::Replaced,
        "read",
    );
}

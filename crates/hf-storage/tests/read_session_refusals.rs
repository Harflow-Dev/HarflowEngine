#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Every documented refusal of the read session, and the wire-table mapping.
mod common;
mod read_session_support;
use hf_storage::{Error, ReadRefusal, SCHEMA_VERSION};
use read_session_support::{
    MAIN, checkpoint, copy_tree, edit_main, expected, open, read_all, seeded,
};
use rusqlite::ffi;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

fn refusal<T>(outcome: hf_storage::Result<T>) -> ReadRefusal {
    match outcome {
        Ok(_) => panic!("the read must be refused"),
        Err(error) => ReadRefusal::from(&error),
    }
}

fn sqlite(code: i32) -> Error {
    Error::Sqlite(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None))
}

#[test]
fn baseline_the_untouched_root_reads() {
    let seeded = seeded(5);
    assert_eq!(read_all(&open(&seeded.root).unwrap()).unwrap().len(), 5);
}

#[test]
fn foreign_application_id_is_a_foreign_store() {
    let seeded = seeded(5);
    checkpoint(&seeded.root);
    let mut main = fs::read(seeded.root.join(MAIN)).unwrap();
    main[68..72].copy_from_slice(&1u32.to_be_bytes());
    fs::write(seeded.root.join(MAIN), main).unwrap();
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::ForeignStore);
}

#[test]
fn marker_generation_mismatch_is_an_invalid_marker() {
    let seeded = seeded(5);
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let version = marker.lines().nth(1).unwrap();
    let other = format!("harflow-storage-2\n{version}\n{}\n", "f".repeat(32));
    assert_ne!(marker, other);
    fs::write(seeded.root.join("store.identity"), other).unwrap();
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::InvalidMarker);
}

#[test]
fn sql_identity_mismatch_is_an_invalid_marker() {
    let seeded = seeded(5);
    edit_main(
        &seeded.root,
        "UPDATE store_identity SET generation=lower(hex(randomblob(16)));",
    );
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::InvalidMarker);
}

#[test]
fn a_nonempty_rollback_journal_is_refused() {
    let seeded = seeded(5);
    fs::write(seeded.root.join("store.sqlite3-journal"), b"pending").unwrap();
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::NonemptyJournal);
    fs::write(seeded.root.join("store.sqlite3-journal"), b"").unwrap();
    assert!(open(&seeded.root).is_ok());
}

#[test]
fn a_newer_schema_in_the_marker_or_the_database_is_refused() {
    let seeded = seeded(5);
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let generation = marker.lines().nth(2).unwrap().to_owned();
    fs::write(
        seeded.root.join("store.identity"),
        format!("harflow-storage-2\n{}\n{generation}\n", SCHEMA_VERSION + 1),
    )
    .unwrap();
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::FutureSchema);
    fs::write(seeded.root.join("store.identity"), marker).unwrap();
    edit_main(
        &seeded.root,
        &format!("PRAGMA user_version={};", SCHEMA_VERSION + 1),
    );
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::FutureSchema);
}

#[test]
fn an_older_schema_in_the_marker_is_an_unsupported_layout() {
    let seeded = seeded(5);
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let generation = marker.lines().nth(2).unwrap().to_owned();
    fs::write(
        seeded.root.join("store.identity"),
        format!("harflow-storage-2\n{}\n{generation}\n", SCHEMA_VERSION - 1),
    )
    .unwrap();
    assert_eq!(refusal(open(&seeded.root)), ReadRefusal::UnsupportedLayout);
}

#[test]
fn a_malformed_older_marker_is_an_invalid_marker_not_an_unsupported_layout() {
    for text in [
        format!("harflow-storage-2\n{}\n", SCHEMA_VERSION - 1),
        format!(
            "harflow-storage-2\n{}\nnot-a-generation\n",
            SCHEMA_VERSION - 1
        ),
        format!(
            "harflow-storage-2\n{}\n{}",
            SCHEMA_VERSION - 1,
            "a".repeat(32)
        ),
    ] {
        let seeded = seeded(5);
        fs::write(seeded.root.join("store.identity"), &text).unwrap();
        assert_eq!(
            refusal(open(&seeded.root)),
            ReadRefusal::InvalidMarker,
            "{text:?}"
        );
    }
}

#[test]
fn a_changed_sql_identity_after_open_is_replaced() {
    let seeded = seeded(5);
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 5);
    // A second connection changes the generation stored in the database only; the
    // marker file still matches, so only the in-transaction identity check can notice.
    let other = rusqlite::Connection::open(seeded.root.join(MAIN)).unwrap();
    other
        .execute(
            "UPDATE store_identity SET generation=lower(hex(randomblob(16)))",
            [],
        )
        .unwrap();
    let error = read_all(&session).err().unwrap();
    assert!(matches!(error, Error::Replaced), "{error:?}");
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::Replaced);
    drop(other);
}

#[test]
fn replaced_identity_stays_a_refusal_and_the_session_does_not_reopen() {
    let seeded = seeded(5);
    let session = open(&seeded.root).unwrap();
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let version = marker.lines().nth(1).unwrap();
    fs::write(
        seeded.root.join("store.identity"),
        format!("harflow-storage-2\n{version}\n{}\n", "e".repeat(32)),
    )
    .unwrap();
    for _ in 0..2 {
        let error = read_all(&session).err().unwrap();
        assert!(matches!(error, Error::Replaced), "{error:?}");
        assert_eq!(ReadRefusal::from(&error), ReadRefusal::Replaced);
    }
}

#[test]
fn a_deadline_already_passed_is_refused_as_busy() {
    let seeded = seeded(5);
    let past = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let error = hf_storage::ReadSession::open(
        &seeded.root,
        common::default_limits(),
        past,
        common::lease::lease_port(),
    )
    .err()
    .unwrap();
    assert!(matches!(error, Error::DeadlineExceeded), "{error:?}");
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::Busy);
    let session = open(&seeded.root).unwrap();
    let error = session
        .task_read(
            &hf_domain::task_read::AuthorizedScope::Global,
            &read_session_support::page(),
            past,
        )
        .err()
        .unwrap();
    assert!(matches!(error, Error::DeadlineExceeded), "{error:?}");
}

// The overrun inside an admitted read is pinned by the in-crate tests in
// `tests::read_session_deadline_tests` (they need stage hooks that exist only inside
// the crate). The event-read test there is the one that isolates the closing deadline
// check of `ReadSession::read`; the task-read test is also caught by inner checks.

#[test]
fn zeroed_pages_give_a_typed_refusal_or_the_correct_rows_never_different_ones() {
    let seeded = seeded(80);
    checkpoint(&seeded.root);
    let size = fs::metadata(seeded.root.join(MAIN)).unwrap().len();
    let pages = size / 4096;
    assert!(pages >= 8, "{pages}");
    let want = expected(seeded.rows.clone());
    let mut corrupt = 0;
    for page in 2..=pages {
        let copy = seeded.scratch.path().join(format!("trial-{page}"));
        copy_tree(&seeded.root, &copy);
        let mut main = fs::read(copy.join(MAIN)).unwrap();
        let start = ((page - 1) * 4096) as usize;
        main[start..start + 4096].fill(0);
        fs::write(copy.join(MAIN), main).unwrap();
        match open(&copy).and_then(|session| read_all(&session)) {
            Ok(rows) => assert_eq!(rows, want, "page {page} returned different rows"),
            Err(error) => {
                let refusal = ReadRefusal::from(&error);
                assert!(
                    !matches!(refusal, ReadRefusal::Busy),
                    "page {page}: {error:?}"
                );
                corrupt += usize::from(matches!(refusal, ReadRefusal::Corrupt { .. }));
            }
        }
    }
    assert!(corrupt > 0, "no zeroed page was reported as corruption");
}

#[test]
fn a_store_lock_that_is_a_directory_is_unavailable() {
    // The root identity capture rejects it before any lease is attempted.
    let seeded = seeded(5);
    fs::remove_file(seeded.root.join("store.lock")).unwrap();
    fs::create_dir(seeded.root.join("store.lock")).unwrap();
    assert_eq!(
        refusal(open(&seeded.root)),
        ReadRefusal::Unavailable { file: "store.lock" }
    );
}

#[test]
fn a_store_lock_the_lease_cannot_open_is_unavailable() {
    // The test lease port opens the lock read-write; a mode-000 file makes it answer
    // `LeaseError::Invalid`, the lease-side failure the wire table lists.
    let seeded = seeded(5);
    if !read_session_support::permissions_bind(seeded.scratch.path()) {
        eprintln!("SKIPPED: file permissions do not bind this user (superuser)");
        return;
    }
    let lock = seeded.root.join("store.lock");
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o000)).unwrap();
    let outcome = open(&seeded.root);
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        refusal(outcome),
        ReadRefusal::Unavailable { file: "store.lock" }
    );
}

#[test]
fn an_unreadable_root_is_unavailable_not_empty() {
    let seeded = seeded(3);
    if !read_session_support::permissions_bind(seeded.scratch.path()) {
        eprintln!("SKIPPED: directory permissions do not bind this user (superuser)");
        return;
    }
    fs::set_permissions(&seeded.root, fs::Permissions::from_mode(0o000)).unwrap();
    let outcome = open(&seeded.root);
    fs::set_permissions(&seeded.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        refusal(outcome),
        ReadRefusal::Unavailable { file: "state root" }
    );
}

#[test]
fn every_error_maps_to_its_row_of_the_wire_table() {
    let corrupt = |error: &Error| matches!(ReadRefusal::from(error), ReadRefusal::Corrupt { .. });
    let cases: Vec<(Error, ReadRefusal)> = vec![
        (Error::WalMissing, ReadRefusal::WalMissing),
        (Error::Busy, ReadRefusal::Busy),
        (Error::DeadlineExceeded, ReadRefusal::Busy),
        (sqlite(ffi::SQLITE_BUSY), ReadRefusal::Busy),
        (sqlite(ffi::SQLITE_LOCKED), ReadRefusal::Busy),
        (
            sqlite(ffi::SQLITE_CANTOPEN),
            ReadRefusal::Unavailable {
                file: "store.sqlite3",
            },
        ),
        (
            sqlite(ffi::SQLITE_READONLY),
            ReadRefusal::Unavailable {
                file: "store.sqlite3",
            },
        ),
        (
            Error::InvalidRoot,
            ReadRefusal::Unavailable { file: "state root" },
        ),
        (
            Error::Io(std::io::Error::from_raw_os_error(13)),
            ReadRefusal::Unavailable { file: "state root" },
        ),
        (
            Error::Io(std::io::Error::other("x")),
            ReadRefusal::Other {
                message: "x".to_owned(),
            },
        ),
        (
            Error::ReadUnavailable("store.lock"),
            ReadRefusal::Unavailable { file: "store.lock" },
        ),
        (Error::ForeignStore, ReadRefusal::ForeignStore),
        (Error::UnsupportedLayout, ReadRefusal::UnsupportedLayout),
        (Error::Incompatible, ReadRefusal::InvalidMarker),
        (Error::FutureSchema, ReadRefusal::FutureSchema),
        (Error::NeedsCheckpoint, ReadRefusal::NonemptyJournal),
        (Error::Replaced, ReadRefusal::Replaced),
    ];
    for (error, want) in cases {
        assert_eq!(ReadRefusal::from(&error), want, "{error:?}");
    }
    assert!(corrupt(&sqlite(ffi::SQLITE_CORRUPT)));
    assert!(corrupt(&sqlite(ffi::SQLITE_NOTADB)));
    for other in [
        sqlite(ffi::SQLITE_FULL),
        Error::LimitExceeded,
        Error::InvalidTaskQuery,
    ] {
        assert!(
            matches!(ReadRefusal::from(&other), ReadRefusal::Other { .. }),
            "{other:?}"
        );
    }
}

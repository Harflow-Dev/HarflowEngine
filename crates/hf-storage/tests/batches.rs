#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::{Error, Limits, Mutation, Store};
use rusqlite::{Connection, Error as SqliteError, ErrorCode, OpenFlags};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn read_batches_enforce_rows_and_bytes_and_return_only_requested_records() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let limits = Limits {
        max_rows: 2,
        max_bytes: 16,
    };
    let store = Store::initialize_fixture(&root, limits, deadline()).unwrap();
    store
        .apply_batch(
            &[
                Mutation::Insert {
                    key: "a".into(),
                    value: b"12".to_vec(),
                },
                Mutation::Insert {
                    key: "b".into(),
                    value: b"345".to_vec(),
                },
            ],
            deadline(),
        )
        .unwrap();

    let records = store.read_batch(&["b".into()], deadline()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].key, "b");
    assert_eq!(records[0].value, b"345");
    assert!(matches!(
        store.read_batch(&["a".into(), "b".into(), "missing".into()], deadline()),
        Err(Error::LimitExceeded)
    ));

    drop(store);
    let byte_limited = Store::open(
        &root,
        Limits {
            max_rows: 2,
            max_bytes: 3,
        },
        deadline(),
    )
    .unwrap();
    assert!(matches!(
        byte_limited.read_batch(&["b".into()], deadline()),
        Err(Error::LimitExceeded)
    ));
}

#[test]
fn oversized_mutation_batches_are_rejected_without_writing_any_records() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let limits = Limits {
        max_rows: 1,
        max_bytes: 4,
    };
    let store = Store::initialize_fixture(&root, limits, deadline()).unwrap();

    assert!(matches!(
        store.apply_batch(
            &[
                Mutation::Insert {
                    key: "a".into(),
                    value: b"1".to_vec()
                },
                Mutation::Insert {
                    key: "b".into(),
                    value: b"2".to_vec()
                },
            ],
            deadline(),
        ),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        store.apply_batch(
            &[Mutation::Insert {
                key: "k".into(),
                value: b"1234".to_vec()
            }],
            deadline(),
        ),
        Err(Error::LimitExceeded)
    ));

    let raw = Connection::open_with_flags(
        root.join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let count: u32 = raw
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn committed_delete_insert_and_put_follow_batch_order() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .apply_batch(
            &[
                Mutation::Insert {
                    key: "replace".into(),
                    value: b"old".to_vec(),
                },
                Mutation::Insert {
                    key: "update".into(),
                    value: b"before".to_vec(),
                },
            ],
            deadline(),
        )
        .unwrap();
    store
        .apply_batch(
            &[
                Mutation::Delete {
                    key: "replace".into(),
                },
                Mutation::Insert {
                    key: "replace".into(),
                    value: b"new".to_vec(),
                },
                Mutation::Put {
                    key: "update".into(),
                    value: b"after".to_vec(),
                },
            ],
            deadline(),
        )
        .unwrap();
    drop(store);

    let raw = Connection::open_with_flags(
        root.join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let replace: Vec<u8> = raw
        .query_row(
            "SELECT value FROM fixture_records WHERE key='replace'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let update: Vec<u8> = raw
        .query_row(
            "SELECT value FROM fixture_records WHERE key='update'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(replace, b"new");
    assert_eq!(update, b"after");
}

#[test]
fn constraint_failure_rolls_back_the_entire_batch() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .apply_batch(
            &[Mutation::Insert {
                key: "existing".into(),
                value: b"old".to_vec(),
            }],
            deadline(),
        )
        .unwrap();

    assert!(matches!(
        store.apply_batch(
            &[
                Mutation::Put {
                    key: "existing".into(),
                    value: b"new".to_vec()
                },
                Mutation::Insert {
                    key: "existing".into(),
                    value: b"duplicate".to_vec()
                },
            ],
            deadline(),
        ),
        Err(Error::Sqlite(_))
    ));
    drop(store);
    let reopened = Store::open(&root, default_limits(), deadline()).unwrap();
    assert_eq!(
        reopened
            .read_batch(&["existing".into()], deadline())
            .unwrap()[0]
            .value,
        b"old"
    );
}

#[test]
fn sqlite_write_contention_expires_admission_without_a_late_mutation() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    let lock = Connection::open(root.join("store.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (sender, receiver) = mpsc::channel();
    let writer = thread::spawn(move || {
        let result = store.apply_batch(
            &[Mutation::Insert {
                key: "late".into(),
                value: b"must-not-land".to_vec(),
            }],
            Instant::now() + Duration::from_millis(100),
        );
        sender.send(result).unwrap();
    });

    let result = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    writer.join().unwrap();
    assert!(
        matches!(result, Err(Error::Busy | Error::DeadlineExceeded))
            || matches!(result, Err(Error::Sqlite(SqliteError::SqliteFailure(code, _)))
            if matches!(code.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
    );
    lock.execute_batch("ROLLBACK").unwrap();
    let observer = Store::open(&root, default_limits(), deadline()).unwrap();
    assert!(
        observer
            .read_batch(&["late".into()], deadline())
            .unwrap()
            .is_empty()
    );
}

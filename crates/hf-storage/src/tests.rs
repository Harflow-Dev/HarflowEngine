#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Limits, Store};
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn configured_store_connection_enforces_wal_full_and_foreign_keys() {
    let parent = std::env::temp_dir().join(format!("hf-storage-pragmas-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let connection = store.connection.lock().unwrap();

    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let synchronous: u32 = connection
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .unwrap();
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();

    assert_eq!(journal.to_lowercase(), "wal");
    assert_eq!(synchronous, 2);
    assert_eq!(foreign_keys, 1);
    drop(connection);
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn mutex_saturation_refuses_immediately_without_a_late_write() {
    let parent = std::env::temp_dir().join(format!("hf-storage-admission-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let guard = store.connection.lock().unwrap();

    assert!(matches!(
        store.apply_batch(
            &[super::Mutation::Insert {
                key: "queued".into(),
                value: b"must-not-land".to_vec(),
            }],
            Instant::now() + Duration::from_secs(2),
        ),
        Err(super::Error::Busy)
    ));
    drop(guard);
    assert!(
        store
            .read_batch(&["queued".into()], Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn already_expired_deadline_rejects_without_a_write() {
    let parent = std::env::temp_dir().join(format!("hf-storage-expired-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();

    assert!(matches!(
        store.apply_batch(
            &[super::Mutation::Insert {
                key: "expired".into(),
                value: b"must-not-land".to_vec(),
            }],
            Instant::now() - Duration::from_millis(1),
        ),
        Err(super::Error::DeadlineExceeded)
    ));
    assert!(
        store
            .read_batch(&["expired".into()], Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::duplicate_mod)]

#[path = "capture_tests/support.rs"]
mod support;
use super::super::{Limits, RecoveryReadSession, Store};
use hf_application::recovery::RecoveryStorePort;
use hf_domain::{capture::CaptureProgress, ownership::Id, process::ProcessExit};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};
use support::{Fixture, accepted_binding, deadline};
mod faults;

pub(super) fn readonly(root: &std::path::Path) -> Connection {
    Connection::open_with_flags(root.join("store.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
}

pub(super) fn source_hashes(root: &std::path::Path) -> Vec<(String, Option<Vec<u8>>)> {
    [
        "store.sqlite3",
        "store.identity",
        "store.lock",
        "io.lock",
        "store.sqlite3-wal",
        "store.sqlite3-shm",
    ]
    .iter()
    .map(|name| ((*name).to_string(), file_hash(root, name)))
    .collect()
}

fn file_hash(root: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    match fs::read(root.join(name)) {
        Ok(bytes) => Some(Sha256::digest(bytes).to_vec()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && name.ends_with("-wal") => {
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && name.ends_with("-shm") => {
            None
        }
        Err(error) => panic!("cannot read required source file {name}: {error}"),
    }
}

pub(super) fn seed_terminal(store: &Store, tag: u8) -> Id {
    let binding = accepted_binding(store, tag);
    let id = binding.fence.operation.clone();
    store.capture_begin(&binding, deadline()).unwrap();
    let final_progress = CaptureProgress {
        revision: 1,
        stdout: 5,
        stderr: 4,
        stdout_eof: true,
        stderr_eof: true,
        loss: false,
    };
    store
        .capture_publish(&binding, 0, &final_progress, deadline())
        .unwrap();
    store
        .capture_finish(&binding, 1, ProcessExit::Code(7), deadline())
        .unwrap();
    id
}

pub(super) fn event_oracle(db: &Connection, operation: &Id) -> (u32, String) {
    let mut stmt = db.prepare("SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_events WHERE operation=?1 ORDER BY revision").unwrap();
    let rows = stmt
        .query_map([operation.as_str()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })
        .unwrap();
    let count = rows.count() as u32;
    // Hash all eight canonical integer fields, independently of the storage validator.
    let mut stmt = db.prepare("SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_events WHERE operation=?1 ORDER BY revision").unwrap();
    let rows = stmt
        .query_map([operation.as_str()], |r| {
            Ok((0..8)
                .map(|i| r.get::<_, i64>(i).unwrap())
                .collect::<Vec<_>>())
        })
        .unwrap();
    let mut hash = Sha256::new();
    for row in rows {
        for field in row.unwrap() {
            hash.update(field.to_be_bytes());
        }
    }
    (count, format!("{:x}", hash.finalize()))
}

#[test]
fn terminal_recovery_snapshot_matches_readonly_sqlite_and_preserves_source_files() {
    let fixture = Fixture::new("recovery-terminal");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 31);
    drop(store);
    let before = source_hashes(&root);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    let record = session.read(&operation, deadline()).unwrap().unwrap();
    assert_eq!(before, source_hashes(&root));
    let raw = readonly(&root);
    let (count, digest) = event_oracle(&raw, &operation);
    assert_eq!(record.events_count, count);
    assert_eq!(record.events_digest.as_str(), digest);
    assert_eq!(record.ownership.request.operation, operation);
    assert_eq!(record.capture.unwrap().exit, ProcessExit::Code(7));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    drop(raw);
}

#[test]
fn absent_invalid_and_expired_sources_refuse_without_initialization() {
    let fixture = Fixture::new("recovery-refusal");
    let root = fixture.root();
    let session = RecoveryReadSession::new(
        &root,
        root.parent().unwrap().join("scratch"),
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    assert_eq!(session.read(&support::id(41), deadline()).unwrap(), None);
    assert!(!root.exists());
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    drop(store);
    assert!(matches!(
        session.read(&support::id(41), Instant::now() - Duration::from_millis(1)),
        Err(hf_application::recovery::RecoveryError::DeadlineExceeded)
    ));
    let malformed = root.clone();
    let mut header = fs::read(malformed.join("store.sqlite3")).unwrap();
    header[0] = b'X';
    fs::write(malformed.join("store.sqlite3"), header).unwrap();
    let _ = fs::remove_file(malformed.join("store.sqlite3-wal"));
    let malformed_session = RecoveryReadSession::new(
        &malformed,
        malformed.parent().unwrap().join("scratch2"),
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    let result = malformed_session.read(&support::id(41), deadline());
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn oversized_event_cell_is_invalid_without_source_repair() {
    let fixture = Fixture::new("recovery-cell");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 61);
    drop(store);
    let write = Connection::open(root.join("store.sqlite3")).unwrap();
    write
        .execute(
            "UPDATE capture_events SET stdout=?2 WHERE operation=?1 AND revision=1",
            rusqlite::params![operation.as_str(), vec![b'x'; 1024 * 1024]],
        )
        .unwrap();
    drop(write);
    let before = fs::read(root.join("store.sqlite3")).unwrap();
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    let result = session.read(&operation, deadline());
    assert!(result.is_err(), "{result:?}");
    assert_eq!(fs::read(root.join("store.sqlite3")).unwrap(), before);
}

#[test]
fn oversized_ownership_version_text_and_blob_are_bounded_and_refused() {
    let fixture = Fixture::new("recovery-meta-cell");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 62);
    drop(store);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    for bad in [
        rusqlite::types::Value::Text("x".repeat(1024 * 1024)),
        rusqlite::types::Value::Blob(vec![b'x'; 1024 * 1024]),
    ] {
        let write = Connection::open(root.join("store.sqlite3")).unwrap();
        write
            .execute("UPDATE ownership_meta SET version=?1", [bad])
            .unwrap();
        drop(write);
        let before = source_hashes(&root);
        assert!(session.read(&operation, deadline()).is_err());
        assert_eq!(before, source_hashes(&root));
        assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
        let restore = Connection::open(root.join("store.sqlite3")).unwrap();
        restore
            .execute("UPDATE ownership_meta SET version=1", [])
            .unwrap();
    }
}

#[test]
fn held_exclusive_io_gate_returns_busy_then_allows_a_fresh_observation() {
    let fixture = Fixture::new("recovery-busy");
    let store = fixture.store();
    let operation = seed_terminal(&store, 51);
    drop(store);
    let io = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.root().join("io.lock"))
        .unwrap();
    assert!(io.try_lock().is_ok());
    let scratch = fixture.root().parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        fixture.root(),
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    assert!(matches!(
        session.read(&operation, deadline()),
        Err(hf_application::recovery::RecoveryError::Busy)
    ));
    io.unlock().unwrap();
    let result = session.read(&operation, deadline());
    assert!(result.is_ok(), "{result:?}");
    assert!(result.unwrap().is_some());
}

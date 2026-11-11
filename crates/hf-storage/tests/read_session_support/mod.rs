#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared fixtures for the read-session tests. Expected values are generated here,
//! independently of the storage code that serves them back.

use crate::common::{self, Fixture, default_limits};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{ReadSession, Store, TaskFixture};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const WAL: &str = "store.sqlite3-wal";
pub const SHM: &str = "store.sqlite3-shm";
pub const MAIN: &str = "store.sqlite3";

pub fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

/// A deterministic row whose every field depends on `number` and `generation`.
pub fn row(generation: u32, number: u32) -> TaskRow {
    TaskRow {
        id: TaskId::new(format!("g{generation:04}-task-{number:04}")).unwrap(),
        stamp: i64::from(generation) * 1000 + i64::from(number),
        session: (!number.is_multiple_of(3)).then(|| format!("session-{}", number % 3)),
        observation: StoredObservation {
            recorded_status: Some(format!("status-{generation}-{number}")),
            active: Some(number.is_multiple_of(2)),
            exited: (number.is_multiple_of(5)).then_some(true),
            drained: Some(number.is_multiple_of(7)),
            owned: None,
        },
        header: format!("header|{generation}|{number}|{}", "h".repeat(600)).into_bytes(),
        status: (!number.is_multiple_of(4))
            .then(|| format!("status|{generation}|{number}").into_bytes()),
    }
}

pub fn rows(generation: u32, count: u32) -> Vec<TaskRow> {
    (0..count).map(|number| row(generation, number)).collect()
}

/// The order the page query promises: stamp descending, then id descending.
pub fn expected(mut rows: Vec<TaskRow>) -> Vec<TaskRow> {
    rows.sort_by(|a, b| (b.stamp, b.id.as_str()).cmp(&(a.stamp, a.id.as_str())));
    rows
}

pub fn fixture_of(rows: Vec<TaskRow>) -> TaskFixture {
    TaskFixture {
        rows,
        launches: Vec::new(),
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}

pub fn page() -> TaskQuery {
    TaskQuery::page(100, PageFilter::new(false, None).unwrap(), None).unwrap()
}

pub struct Seeded {
    pub scratch: Fixture,
    pub root: PathBuf,
    pub rows: Vec<TaskRow>,
}

/// A root built by the real initializer whose rows live only in the WAL.
pub fn seeded(count: u32) -> Seeded {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    let rows = rows(1, count);
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    store
        .replace_task_fixture(&fixture_of(rows.clone()), later(5))
        .unwrap();
    drop(store);
    Seeded {
        scratch,
        root,
        rows,
    }
}

pub fn open(root: &Path) -> hf_storage::Result<ReadSession> {
    ReadSession::open(
        root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
}

pub fn read_all(session: &ReadSession) -> hf_storage::Result<Vec<TaskRow>> {
    session
        .task_read(&AuthorizedScope::Global, &page(), later(5))
        .map(|result| result.rows)
}

pub fn bytes(root: &Path, name: &str) -> Vec<u8> {
    fs::read(root.join(name)).unwrap()
}

/// Folds the WAL into the main file the way an out-of-protocol client would.
pub fn checkpoint(root: &Path) {
    let connection = Connection::open(root.join(MAIN)).unwrap();
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    drop(connection);
    // The last close of a non-engine client removes -wal; a restore emits an empty one.
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join(WAL), b"").unwrap();
}

pub fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// True when directory permission bits actually bind this process (not the superuser).
pub fn permissions_bind(scratch: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let probe = scratch.join("permission-probe");
    fs::create_dir(&probe).unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o555)).unwrap();
    let binds = fs::File::create(probe.join("x")).is_err();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_dir_all(&probe).unwrap();
    binds
}

/// Edits the main file with a plain client, then restores the engine shape (empty `-wal`).
pub fn edit_main(root: &Path, sql: &str) {
    let connection = Connection::open(root.join(MAIN)).unwrap();
    connection.execute_batch(sql).unwrap();
    drop(connection);
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join(WAL), b"").unwrap();
}

pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

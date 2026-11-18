#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Helpers for the WAL checkpoint-owner tests. Sizes come from the file system; the
//! large values are written through the ordinary fixture batch writer.

use crate::common::{self, deadline, default_limits};
use hf_storage::{Mutation, Store, WalMaintenance, WriterOptions};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const WAL: &str = "store.sqlite3-wal";
pub const MAIN: &str = "store.sqlite3";
/// One large value: just under the default batch byte limit with its key.
pub const BULK: usize = 1_000_000;
/// Upper bound on the `-wal` growth of one `BULK` commit (overflow pages plus frame headers).
pub const ONE_COMMIT: u64 = 1_200_000;

/// A root built by the real initializer, with no handle left open.
pub fn initialized(scratch: &common::Fixture) -> PathBuf {
    let root = scratch.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    root
}

pub fn open(root: &Path, options: WriterOptions) -> Store {
    Store::open_with(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
        options,
    )
    .unwrap()
}

pub fn owner(root: &Path) -> Store {
    open(root, WriterOptions::owner())
}

/// One commit replacing `key` with `len` bytes of `fill`.
pub fn put(store: &Store, key: &str, len: usize, fill: u8) {
    store
        .apply_batch(
            &[Mutation::Put {
                key: key.into(),
                value: vec![fill; len],
            }],
            deadline(),
        )
        .unwrap();
}

/// `count` commits of one `BULK` value each, all to the same key.
pub fn bulk(store: &Store, count: u8) {
    for fill in 0..count {
        put(store, "bulk", BULK, fill);
    }
}

pub fn wal_len(root: &Path) -> u64 {
    let metadata = fs::symlink_metadata(root.join(WAL)).unwrap();
    assert!(metadata.is_file(), "-wal must be a regular file");
    metadata.len()
}

pub fn bytes(root: &Path, name: &str) -> Vec<u8> {
    fs::read(root.join(name)).unwrap()
}

/// A raw reader holding an open read transaction: it pins every later WAL frame.
pub fn pin(root: &Path) -> Connection {
    let reader = common::raw_connection(root.join(MAIN));
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let _: i64 = reader
        .query_row("SELECT count(*) FROM fixture_records", [], |row| row.get(0))
        .unwrap();
    reader
}

pub fn release(reader: Connection) {
    reader.execute_batch("COMMIT").unwrap();
    drop(reader);
}

/// The value stored under `key`, read back through the store.
pub fn value(store: &Store, key: &str) -> Vec<u8> {
    store
        .read_batch(&[key.into()], deadline())
        .unwrap()
        .pop()
        .unwrap()
        .value
}

/// A handle with `owner_idle()` and the given copy budget.
pub fn idle_with(root: &Path, budget: std::time::Duration) -> Store {
    let mut options = WriterOptions::owner_idle();
    options.passive_budget = budget;
    open(root, options)
}

pub fn owner_idle(root: &Path) -> Store {
    open(root, WriterOptions::owner_idle())
}

/// `count` bulk commits through a Suppressed handle, which is then dropped: a backlog of
/// roughly `count` MiB that nothing has checkpointed.
pub fn suppressed_fill(root: &Path, count: u8) {
    let store = open(root, WriterOptions::suppressed());
    bulk(&store, count);
}

/// (frames in the log, frames checkpointed) as a raw NOOP checkpoint reports them.
pub fn noop(root: &Path) -> (i64, i64) {
    let connection = common::raw_connection(root.join(MAIN));
    connection
        .query_row("PRAGMA wal_checkpoint(NOOP)", [], |row| {
            Ok((row.get(1)?, row.get(2)?))
        })
        .unwrap()
}

pub fn sequence(root: &Path) -> i64 {
    let connection = common::raw_connection(root.join(MAIN));
    connection
        .query_row("SELECT value FROM commit_sequence", [], |row| row.get(0))
        .unwrap()
}

pub const SHRUNK: WalMaintenance = WalMaintenance::Checkpointed {
    complete: true,
    shrink_commit_landed: true,
};
pub const DONE: WalMaintenance = WalMaintenance::Checkpointed {
    complete: true,
    shrink_commit_landed: false,
};

/// `batches` Suppressed commits of 64 distinct 1 MB values each (about `64 * batches` MiB of
/// distinct pages): a backlog whose copy time grows with its size, unlike repeated updates
/// of one key, which a checkpoint copies once.
pub fn distinct_fill(root: &Path, batches: u8) {
    let limits = hf_storage::Limits {
        max_rows: 128,
        max_bytes: 70_000_000,
    };
    let store = Store::open_with(
        root,
        limits,
        deadline(),
        common::lease::lease_port(),
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
        store
            .apply_batch(&mutations, deadline() + std::time::Duration::from_secs(60))
            .unwrap();
    }
}

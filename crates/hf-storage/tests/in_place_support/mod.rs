#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Builders shared by the in-place qualification tests.

use crate::common::{self, Fixture, deadline, default_limits};
use hf_storage::Store;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MIB: usize = 1024 * 1024;
pub const MAIN: &str = "store.sqlite3";
pub const WAL: &str = "store.sqlite3-wal";
pub const SHM: &str = "store.sqlite3-shm";

pub struct Seeded {
    pub scratch: Fixture,
    pub root: PathBuf,
}

pub fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

/// A freshly initialized store with a small committed history in its WAL.
pub fn small_store() -> Seeded {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    store
        .apply_batch(
            &[hf_storage::Mutation::Put {
                key: "small".into(),
                value: b"value".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    drop(store);
    Seeded { scratch, root }
}

fn insert_blobs(connection: &rusqlite::Connection, prefix: &str, count: usize, bytes: usize) {
    let transaction = connection.unchecked_transaction().unwrap();
    for number in 0..count {
        transaction
            .execute(
                "INSERT INTO fixture_records(key,value) VALUES(?1, zeroblob(?2))",
                rusqlite::params![format!("{prefix}-{number}"), bytes as i64],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
}

/// A store whose main file and `-wal` together exceed 64 MiB while neither does:
/// 40 MiB is folded into the main file, then 40 MiB more stays in the WAL. The
/// blobs are `zeroblob`s written in two transactions, so building it stays fast.
pub fn large_store() -> Seeded {
    let seeded = small_store();
    let connection = common::raw_connection(seeded.root.join(MAIN));
    insert_blobs(&connection, "folded", 5, 8 * MIB);
    connection
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))
        .unwrap();
    insert_blobs(&connection, "logged", 5, 8 * MIB);
    drop(connection);
    let main = fs::metadata(seeded.root.join(MAIN)).unwrap().len();
    let wal = fs::metadata(seeded.root.join(WAL)).unwrap().len();
    let cap = 64 * MIB as u64;
    assert!(
        main + wal > cap,
        "main {main} + wal {wal} must exceed the old cap"
    );
    assert!(main < cap && wal < cap + 8 * MIB as u64);
    seeded
}

/// A store with many small rows: the b-tree and index checks of `quick_check`
/// take far longer than a scan over a few large overflow chains.
pub fn row_heavy_store() -> Seeded {
    let seeded = small_store();
    let connection = common::raw_connection(seeded.root.join(MAIN));
    connection
        .execute(
            "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c WHERE n<?1) \
             INSERT INTO fixture_records(key,value) SELECT 'row-'||n, zeroblob(64) FROM c",
            [1_500_000_i64],
        )
        .unwrap();
    // Move the bulk into the main file and keep a small WAL tail so `-wal` exists.
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    connection
        .execute(
            "INSERT INTO fixture_records VALUES('tail', zeroblob(8))",
            [],
        )
        .unwrap();
    drop(connection);
    seeded
}

/// Names under `directory`, sorted.
pub fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// True when any `.hf-snapshot-*` directory sits in `parent` or inside `root`.
pub fn has_snapshot_dir(root: &Path) -> bool {
    [root.parent().unwrap(), root].iter().any(|directory| {
        names(directory)
            .iter()
            .any(|name| name.starts_with(".hf-snapshot-"))
    })
}

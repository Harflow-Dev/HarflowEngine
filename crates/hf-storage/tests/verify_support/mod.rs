#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Helpers for the whole-store verification tests. Fence and due-record bytes are
//! written here directly, independently of the storage code that reads them.

use crate::common::{self, default_limits};
use crate::read_session_support::{MAIN, SHM, Seeded, WAL, later, seeded};
use hf_storage::{Error, ReadRefusal, Store, probe};
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const FENCE: &str = "store.fence";
pub const VERIFY: &str = "store.verify";
pub const PERIOD: u64 = 24 * 60 * 60;

/// The generation the root's marker names.
pub fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

pub fn fence_bytes(generation: &str, state: &str) -> Vec<u8> {
    format!("harflow-fence-1\n{generation}\n{state}\n").into_bytes()
}

pub fn due_bytes(generation: &str, due: &str) -> Vec<u8> {
    format!("harflow-verify-1\n{generation}\n{due}\n").into_bytes()
}

/// A wall-clock instant a whole number of seconds after the epoch.
pub fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

pub fn open_store(root: &Path) -> hf_storage::Result<Store> {
    Store::open(
        root,
        default_limits(),
        later(30),
        common::lease::lease_port(),
    )
}

/// Removes whatever occupies `name`: file, symlink or directory.
pub fn clear(root: &Path, name: &str) {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir(&path).unwrap(),
        Ok(_) => fs::remove_file(&path).unwrap(),
        Err(_) => {}
    }
}

pub fn exists(root: &Path, name: &str) -> bool {
    fs::symlink_metadata(root.join(name)).is_ok()
}

/// Names in `root` starting with `store.fence` or `store.verify`, temporaries included.
pub fn verification_files(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(FENCE) || name.starts_with(VERIFY))
        .collect();
    names.sort();
    names
}

/// A seeded root (three task rows) whose main file has one overflow page damaged.
/// Writer and read-session opens verify identity only, so both still accept it.
pub fn damaged() -> Seeded {
    let seeded = seeded(3);
    let root = &seeded.root;
    {
        let connection = rusqlite::Connection::open(root.join(MAIN)).unwrap();
        connection
            .execute(
                "INSERT INTO fixture_records(key,value) VALUES('blob', ?1)",
                [vec![0xAB_u8; 400_000]],
            )
            .unwrap();
        connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
    }
    damage_overflow_page(root);
    seeded
}

/// Points one overflow page in the middle of the blob at a page that does not exist.
fn damage_overflow_page(root: &Path) {
    let mut bytes = fs::read(root.join(MAIN)).unwrap();
    let page = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
    let page = if page == 1 { 65_536 } else { page };
    let marked: Vec<usize> = (3..bytes.len() / page)
        .filter(|number| {
            bytes[number * page + 4..(number + 1) * page]
                .iter()
                .all(|b| *b == 0xAB)
        })
        .collect();
    assert!(
        marked.len() > 4,
        "the blob must span several overflow pages"
    );
    let victim = marked[marked.len() / 2] * page;
    bytes[victim..victim + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(root.join(MAIN), bytes).unwrap();
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join(WAL), b"").unwrap();
}

/// A store with many small rows, so `quick_check` runs long enough to interrupt,
/// kept far smaller than the in-place scan fixture. A small WAL tail keeps `-wal`.
pub fn row_heavy(rows: i64) -> Seeded {
    let seeded = seeded(3);
    let connection = common::raw_connection(seeded.root.join(MAIN));
    connection
        .execute(
            "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c WHERE n<?1) \
             INSERT INTO fixture_records(key,value) SELECT 'row-'||n, zeroblob(8) FROM c",
            [rows],
        )
        .unwrap();
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

type Entry = fn(&Path) -> hf_storage::Result<()>;

fn open_entry(root: &Path) -> hf_storage::Result<()> {
    open_store(root).map(drop)
}

fn probe_entry(root: &Path) -> hf_storage::Result<()> {
    probe(root, common::lease::lease_port()).map(drop)
}

fn recover_entry(root: &Path) -> hf_storage::Result<()> {
    Store::recover(root, later(10), common::lease::lease_port()).map(drop)
}

fn session_entry(root: &Path) -> hf_storage::Result<()> {
    crate::read_session_support::open(root).map(drop)
}

/// Writer open, probe, recover and read-session open.
pub const ENTRIES: [(&str, Entry); 4] = [
    ("open", open_entry),
    ("probe", probe_entry),
    ("recover", recover_entry),
    ("read session", session_entry),
];

/// Every entry refuses with an error `want` accepts and that maps to `refusal`.
pub fn assert_entries_refuse(
    root: &Path,
    want: fn(&Error) -> bool,
    refusal: &ReadRefusal,
    label: &str,
) {
    for (name, entry) in ENTRIES {
        match entry(root) {
            Ok(()) => panic!("{label}: {name} must refuse"),
            Err(error) => {
                assert!(want(&error), "{label}: {name}: {error:?}");
                assert_eq!(&ReadRefusal::from(&error), refusal, "{label}: {name}");
            }
        }
    }
}

pub fn is_fenced(error: &Error) -> bool {
    matches!(error, Error::Fenced)
}

pub fn is_incompatible(error: &Error) -> bool {
    matches!(error, Error::Incompatible)
}

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Helpers for the lost-WAL acceptance record tests. The record bytes are written
//! here directly, independently of the storage code that reads them.

use crate::common::{self, default_limits};
use crate::read_session_support::{later, open, read_all};
use hf_storage::{Error, ReadRefusal, Store, probe};
use std::{fs, path::Path};

pub const RECORD: &str = "store.wal-acceptance";

type Entry = fn(&Path) -> hf_storage::Result<()>;

fn open_store(root: &Path) -> hf_storage::Result<()> {
    Store::open(
        root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .map(drop)
}

fn probe_store(root: &Path) -> hf_storage::Result<()> {
    probe(root, common::lease::lease_port()).map(drop)
}

fn recover_store(root: &Path) -> hf_storage::Result<()> {
    Store::recover(root, later(5), common::lease::lease_port()).map(drop)
}

fn open_session(root: &Path) -> hf_storage::Result<()> {
    open(root).map(drop)
}

/// Writer open, probe, recover and read-session open: the four entries that qualify a root.
pub const ENTRIES: [(&str, Entry); 4] = [
    ("open", open_store),
    ("probe", probe_store),
    ("recover", recover_store),
    ("read session", open_session),
];

/// The generation the root's marker names.
pub fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

/// The exact bytes of a well-formed record.
pub fn record_bytes(generation: &str, state: &str) -> Vec<u8> {
    format!("harflow-wal-acceptance-1\n{generation}\n{state}\n").into_bytes()
}

/// Writes a well-formed record for the root's own generation.
pub fn put_state(root: &Path, state: &str) {
    fs::write(root.join(RECORD), record_bytes(&generation(root), state)).unwrap();
}

pub fn put_bytes(root: &Path, bytes: &[u8]) {
    fs::write(root.join(RECORD), bytes).unwrap();
}

/// Removes whatever occupies the record name: file, symlink, FIFO or directory.
pub fn clear(root: &Path) {
    let path = root.join(RECORD);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir(&path).unwrap(),
        Ok(_) => fs::remove_file(&path).unwrap(),
        Err(_) => {}
    }
}

pub fn make_fifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Asserts `outcome` is an error that `want` accepts and that maps to `refusal`.
pub fn assert_refused(
    outcome: hf_storage::Result<()>,
    want: fn(&Error) -> bool,
    refusal: &ReadRefusal,
    label: &str,
) {
    match outcome {
        Ok(()) => panic!("{label}: the root must be refused"),
        Err(error) => {
            assert!(want(&error), "{label}: {error:?}");
            assert_eq!(&ReadRefusal::from(&error), refusal, "{label}");
        }
    }
}

pub fn is_incompatible(error: &Error) -> bool {
    matches!(error, Error::Incompatible)
}

pub fn is_wal_missing(error: &Error) -> bool {
    matches!(error, Error::WalMissing)
}

/// Every entry, then a `read()` on a session opened before the defect existed, must
/// refuse once `plant` has put the defect in place.
pub fn assert_refused_everywhere(
    root: &Path,
    plant: impl Fn(&Path),
    want: fn(&Error) -> bool,
    refusal: &ReadRefusal,
    label: &str,
) {
    plant(root);
    for (name, entry) in ENTRIES {
        assert_refused(entry(root), want, refusal, &format!("{label}: {name}"));
    }
    clear(root);
    let session = open(root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 3, "{label}: baseline");
    plant(root);
    assert_refused(
        read_all(&session).map(drop),
        want,
        refusal,
        &format!("{label}: read"),
    );
}

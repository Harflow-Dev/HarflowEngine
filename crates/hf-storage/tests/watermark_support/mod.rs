#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Helpers for the commit watermark tests: a recording flush port, slot edits written
//! with the shared codec, and small commit and read wrappers.

use crate::common::{self, deadline, default_limits};
use hf_application::{
    durability::FlushPort,
    watermark::{self, BLOCK_LEN, Decoded, FILE_LEN, NAME, PAYLOAD_LEN, Slot},
};
use hf_storage::{Error, Mutation, ReadSession, Store, WriterOptions};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Counts flushes and keeps a copy of the file as each flush saw it: a power loss keeps
/// exactly the last image a successful flush made durable. The flush numbered `fail_at`
/// (1-based) and every later one fail; a failed flush makes nothing durable. The
/// initializer's descriptor is write-only, so that image is read through `root`.
#[derive(Debug)]
pub struct Recorder {
    pub flushes: AtomicUsize,
    fail_at: AtomicUsize,
    images: Mutex<Vec<(bool, Vec<u8>)>>,
    root: Mutex<Option<PathBuf>>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            flushes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(usize::MAX),
            images: Mutex::new(Vec::new()),
            root: Mutex::new(None),
        }
    }
}

impl Recorder {
    pub fn count(&self) -> usize {
        self.flushes.load(Ordering::SeqCst)
    }
    /// The next `more` flushes succeed and every later one fails.
    pub fn fail_after(&self, more: usize) {
        self.fail_at
            .store(self.count() + more + 1, Ordering::SeqCst);
    }
    pub fn heal(&self) {
        self.fail_at.store(usize::MAX, Ordering::SeqCst);
    }
    /// Every image seen at a flush, with whether that flush succeeded.
    pub fn images(&self) -> Vec<(bool, Vec<u8>)> {
        self.images.lock().unwrap().clone()
    }
    /// The image of the last successful flush: what survives a power loss now.
    pub fn durable(&self) -> Option<Vec<u8>> {
        self.images()
            .into_iter()
            .rev()
            .find_map(|(ok, image)| ok.then_some(image))
    }
}

impl FlushPort for Recorder {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        let number = self.flushes.fetch_add(1, Ordering::SeqCst) + 1;
        let mut image = vec![0u8; FILE_LEN];
        match file.read_at(&mut image, 0) {
            Ok(read) => image.truncate(read),
            Err(_) => {
                let root = self
                    .root
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("root for a write-only file");
                image = fs::read(root.join(NAME))?;
            }
        }
        let ok = number < self.fail_at.load(Ordering::SeqCst);
        self.images.lock().unwrap().push((ok, image));
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::other("injected flush failure"))
        }
    }
}

pub struct Root {
    pub scratch: common::Fixture,
    pub root: PathBuf,
}

/// A fresh root with a writer handle whose watermark flush is `port`.
pub fn initialized(port: Arc<Recorder>) -> (Root, Store) {
    let scratch = common::Fixture::new();
    let root = scratch.path().join("state");
    *port.root.lock().unwrap() = Some(root.clone());
    let options = WriterOptions::suppressed().with_flush(port);
    let store = Store::initialize_fixture_with(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
        options,
    )
    .unwrap();
    (Root { scratch, root }, store)
}

pub fn reopen(root: &Path, port: Arc<Recorder>) -> hf_storage::Result<Store> {
    Store::open_with(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
        WriterOptions::suppressed().with_flush(port),
    )
}

pub fn session(root: &Path) -> hf_storage::Result<ReadSession> {
    ReadSession::open(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
}

pub fn put(store: &Store, key: &str) -> Result<(), Error> {
    let mutation = Mutation::Put {
        key: key.to_string(),
        value: key.as_bytes().to_vec(),
    };
    store.apply_batch(&[mutation], deadline())
}

/// The keys of `keys` that the handle reads back.
pub fn present(store: &Store, keys: &[&str]) -> Vec<String> {
    let keys: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
    store
        .read_batch(&keys, deadline())
        .unwrap()
        .into_iter()
        .map(|record| record.key)
        .collect()
}

pub fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

pub fn file_bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join(NAME)).unwrap()
}

pub fn decoded(root: &Path) -> Decoded {
    watermark::decode(&file_bytes(root), &generation(root))
}

pub fn at(best: Slot, seq: u64) -> Decoded {
    Decoded::Valid { best, seq }
}

/// The sequence stored in the database, read with a raw read-only connection.
pub fn database_seq(root: &Path) -> u64 {
    let connection = rusqlite::Connection::open_with_flags(
        root.join("store.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let value: i64 = connection
        .query_row(
            "SELECT value FROM commit_sequence WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    u64::try_from(value).unwrap()
}

fn slot_file(root: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .open(root.join(NAME))
        .unwrap()
}

/// Writes a valid payload for the root's generation at `seq` into `slot`.
pub fn set_slot(root: &Path, slot: Slot, seq: u64) {
    let payload = watermark::encode_payload(&generation(root), seq).unwrap();
    slot_file(root)
        .write_all_at(&payload, slot.offset())
        .unwrap();
}

/// Damages `slot` the way a torn write would: the start of a newer payload, through part
/// of its sequence, over the old one.
pub fn tear_slot(root: &Path, slot: Slot) {
    let newer = watermark::encode_payload(&generation(root), u64::MAX / 2).unwrap();
    slot_file(root)
        .write_all_at(&newer[..44], slot.offset())
        .unwrap();
    let block = slot_block(root, slot);
    assert!(
        watermark::decode_blocks(&block, &block, &generation(root)) == Decoded::Unavailable,
        "the torn slot must be invalid"
    );
}

pub fn slot_block(root: &Path, slot: Slot) -> Vec<u8> {
    let bytes = file_bytes(root);
    let start = usize::try_from(slot.offset()).unwrap();
    bytes[start..start + BLOCK_LEN].to_vec()
}

/// A whole file whose blocks are `a` and `b` (each `None` is all zero).
pub fn file_of(generation: &str, a: Option<u64>, b: Option<u64>) -> Vec<u8> {
    let mut bytes = vec![0u8; FILE_LEN];
    for (slot, seq) in [(Slot::A, a), (Slot::B, b)] {
        if let Some(seq) = seq {
            let start = usize::try_from(slot.offset()).unwrap();
            bytes[start..start + PAYLOAD_LEN]
                .copy_from_slice(&watermark::encode_payload(generation, seq).unwrap());
        }
    }
    bytes
}

/// Replaces the watermark contents in place (same inode), as a restore of the file would.
pub fn write_file(root: &Path, bytes: &[u8]) {
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(root.join(NAME))
        .unwrap();
    file.write_all_at(bytes, 0).unwrap();
}

/// Copies the database and its log to `saved` (the root must have no open handle).
pub fn save_database(root: &Path, saved: &Path) {
    fs::create_dir_all(saved).unwrap();
    for name in ["store.sqlite3", "store.sqlite3-wal"] {
        fs::copy(root.join(name), saved.join(name)).unwrap();
    }
}

/// Puts a saved database and log back and drops the shared-memory index.
pub fn restore_database(root: &Path, saved: &Path) {
    for name in ["store.sqlite3", "store.sqlite3-wal"] {
        fs::copy(saved.join(name), root.join(name)).unwrap();
    }
    let _ = fs::remove_file(root.join("store.sqlite3-shm"));
}

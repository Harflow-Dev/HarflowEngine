#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Smoke coverage of the commit watermark: the writer publishes each commit in the
//! non-best slot with a flush before and after the write, a database restored below the
//! watermark is refused as `CommitLost`, and a failed flush poisons only the handle.
mod common;
use common::{Fixture, deadline, default_limits, lease::lease_port};
use hf_application::{
    durability::FlushPort,
    watermark::{self, Decoded, Slot},
};
use hf_storage::{Error, Mutation, ReadSession, Store, WriterOptions};
use std::{
    fs::{self, File},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Counts flushes; the flush numbered `fail_at` (1-based) and every later one fail.
#[derive(Debug)]
struct Counting {
    flushes: AtomicUsize,
    fail_at: AtomicUsize,
}
impl Default for Counting {
    fn default() -> Self {
        Self {
            flushes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(usize::MAX),
        }
    }
}
impl Counting {
    fn fail_after(&self, more: usize) {
        let now = self.flushes.load(Ordering::SeqCst);
        self.fail_at.store(now + more + 1, Ordering::SeqCst);
    }
}
impl FlushPort for Counting {
    fn flush(&self, _: &File) -> std::io::Result<()> {
        let number = self.flushes.fetch_add(1, Ordering::SeqCst) + 1;
        if number >= self.fail_at.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("injected flush failure"));
        }
        Ok(())
    }
}

fn decoded(root: &Path, generation: &str) -> Decoded {
    watermark::decode(&fs::read(root.join(watermark::NAME)).unwrap(), generation)
}

fn put(store: &Store, key: &str) -> Result<(), Error> {
    let mutation = Mutation::Put {
        key: key.to_string(),
        value: b"v".to_vec(),
    };
    store.apply_batch(&[mutation], deadline())
}

fn initialize(root: &Path, port: Arc<Counting>) -> Store {
    let options = WriterOptions::suppressed().with_flush(port);
    Store::initialize_fixture_with(root, default_limits(), deadline(), lease_port(), options)
        .unwrap()
}

#[test]
fn each_commit_writes_the_non_best_slot_between_two_flushes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("state");
    let port = Arc::new(Counting::default());
    let store = initialize(&root, port.clone());
    let generation = store.identity().generation.clone();
    assert_eq!(
        port.flushes.load(Ordering::SeqCst),
        1,
        "initialize flushes once"
    );
    let at = |best, seq| Decoded::Valid { best, seq };
    assert_eq!(decoded(&root, &generation), at(Slot::A, 0));
    put(&store, "one").unwrap();
    assert_eq!(decoded(&root, &generation), at(Slot::B, 1));
    put(&store, "two").unwrap();
    assert_eq!(decoded(&root, &generation), at(Slot::A, 2));
    assert_eq!(port.flushes.load(Ordering::SeqCst), 5);
    assert_eq!(
        fs::metadata(root.join(watermark::NAME)).unwrap().len(),
        8192
    );
}

#[test]
fn a_database_restored_below_the_watermark_is_commit_lost() {
    let fixture = Fixture::new();
    let root = fixture.path().join("state");
    let saved = fixture.path().join("saved");
    fs::create_dir(&saved).unwrap();
    let store = initialize(&root, Arc::new(Counting::default()));
    put(&store, "one").unwrap();
    for name in ["store.sqlite3", "store.sqlite3-wal"] {
        fs::copy(root.join(name), saved.join(name)).unwrap();
    }
    put(&store, "two").unwrap();
    drop(store);
    for name in ["store.sqlite3", "store.sqlite3-wal"] {
        fs::copy(saved.join(name), root.join(name)).unwrap();
    }
    let _ = fs::remove_file(root.join("store.sqlite3-shm"));
    let read = ReadSession::open(&root, default_limits(), deadline(), lease_port());
    assert!(matches!(read, Err(Error::CommitLost)), "{:?}", read.err());
    let open = Store::open(&root, default_limits(), deadline(), lease_port());
    assert!(matches!(open, Err(Error::CommitLost)), "{:?}", open.err());
}

#[test]
fn a_failed_pre_flush_commits_nothing_and_poisons_the_handle() {
    let fixture = Fixture::new();
    let root = fixture.path().join("state");
    let port = Arc::new(Counting::default());
    let store = initialize(&root, port.clone());
    let generation = store.identity().generation.clone();
    port.fail_after(0);
    assert!(matches!(
        put(&store, "one"),
        Err(Error::WatermarkUnrecorded)
    ));
    port.fail_at.store(usize::MAX, Ordering::SeqCst);
    assert!(matches!(
        put(&store, "two"),
        Err(Error::WatermarkUnrecorded)
    ));
    drop(store);
    let at_zero = Decoded::Valid {
        best: Slot::A,
        seq: 0,
    };
    assert_eq!(decoded(&root, &generation), at_zero);
    let reopened = Store::open(&root, default_limits(), deadline(), lease_port()).unwrap();
    put(&reopened, "three").unwrap();
}

#[test]
fn a_failed_post_flush_is_an_unknown_outcome_and_reopen_reads_the_commit() {
    let fixture = Fixture::new();
    let root = fixture.path().join("state");
    let port = Arc::new(Counting::default());
    let store = initialize(&root, port.clone());
    port.fail_after(1);
    assert!(matches!(
        put(&store, "one"),
        Err(Error::CommitOutcomeUnknown)
    ));
    port.fail_at.store(usize::MAX, Ordering::SeqCst);
    assert!(matches!(
        put(&store, "two"),
        Err(Error::WatermarkUnrecorded)
    ));
    let keys = ["one".to_string(), "two".to_string()];
    assert_eq!(store.read_batch(&keys, deadline()).unwrap().len(), 1);
    drop(store);
    ReadSession::open(&root, default_limits(), deadline(), lease_port()).unwrap();
    let reopened = Store::open(&root, default_limits(), deadline(), lease_port()).unwrap();
    put(&reopened, "three").unwrap();
}

#[test]
fn a_refused_flush_at_initialize_is_io_and_leaves_no_openable_store() {
    let fixture = Fixture::new();
    let root = fixture.path().join("state");
    let port = Arc::new(Counting::default());
    port.fail_after(0);
    let options = WriterOptions::suppressed().with_flush(port);
    let created =
        Store::initialize_fixture_with(&root, default_limits(), deadline(), lease_port(), options);
    assert!(matches!(created, Err(Error::Io(_))), "{:?}", created.err());
    let open = Store::open(&root, default_limits(), deadline(), lease_port());
    assert!(open.is_err(), "a root whose watermark flush failed opened");
    let options = WriterOptions::suppressed().with_flush(Arc::new(Counting::default()));
    let retried =
        Store::initialize_fixture_with(&root, default_limits(), deadline(), lease_port(), options);
    assert!(
        matches!(retried, Err(Error::InvalidRoot)),
        "{:?}",
        retried.err()
    );
}

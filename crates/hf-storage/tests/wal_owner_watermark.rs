#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! WAL maintenance and the commit watermark: a poisoned handle still runs its PASSIVE
//! checkpoint, and the maintenance commit that follows surfaces `WatermarkUnrecorded` or
//! `CommitLost` instead of reporting a skip.
mod common;
mod wal_owner_support;
mod watermark_support;
use common::{Fixture, deadline};
use hf_application::watermark::Decoded;
use hf_storage::{Error, JOURNAL_SIZE_LIMIT, Mutation, Store, WriterOptions};
use std::sync::Arc;
use wal_owner_support::{MAIN, bulk, bytes, initialized, open, pin, release, wal_len};
use watermark_support::{Recorder, database_seq, decoded, set_slot};

/// An Owner handle flushing through `port`, with a log grown past the limit while a
/// reader pinned it, so the next maintenance checkpoints and then needs its commit.
fn grown(scratch: &Fixture, port: Arc<Recorder>) -> (std::path::PathBuf, Store) {
    let root = initialized(scratch);
    let store = open(&root, WriterOptions::owner().with_flush(port));
    let reader = pin(&root);
    bulk(&store, 18);
    release(reader);
    assert!(wal_len(&root) > JOURNAL_SIZE_LIMIT);
    (root, store)
}

fn small_put(store: &Store) -> hf_storage::Result<()> {
    let mutation = Mutation::Put {
        key: "small".into(),
        value: b"v".to_vec(),
    };
    store.apply_batch(&[mutation], deadline())
}

#[test]
fn a_poisoned_handle_checkpoints_and_then_reports_the_unrecorded_watermark() {
    let scratch = Fixture::new();
    let port = Arc::new(Recorder::default());
    let (root, store) = grown(&scratch, port.clone());
    port.fail_after(0);
    assert!(matches!(small_put(&store), Err(Error::WatermarkUnrecorded)));
    port.heal();
    let seq = database_seq(&root);
    let main = bytes(&root, MAIN);
    let result = store.maintain_wal(deadline());
    assert!(
        matches!(result, Err(Error::WatermarkUnrecorded)),
        "{result:?}"
    );
    assert_ne!(bytes(&root, MAIN), main, "the PASSIVE checkpoint ran");
    assert_eq!(database_seq(&root), seq, "no maintenance commit landed");
    assert!(matches!(small_put(&store), Err(Error::WatermarkUnrecorded)));
}

#[test]
fn a_lost_commit_found_by_the_maintenance_commit_is_reported() {
    let scratch = Fixture::new();
    let (root, store) = grown(&scratch, Arc::new(Recorder::default()));
    let seq = database_seq(&root);
    let Decoded::Valid { best, .. } = decoded(&root) else {
        panic!("no valid slot");
    };
    set_slot(&root, best.other(), seq + 5);
    let main = bytes(&root, MAIN);
    let result = store.maintain_wal(deadline());
    assert!(matches!(result, Err(Error::CommitLost)), "{result:?}");
    assert_ne!(bytes(&root, MAIN), main, "the PASSIVE checkpoint ran");
    assert_eq!(database_seq(&root), seq);
    drop(store);
    assert!(matches!(
        Store::open(
            &root,
            common::default_limits(),
            deadline(),
            common::lease::lease_port()
        ),
        Err(Error::CommitLost)
    ));
}

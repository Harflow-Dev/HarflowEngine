#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The `commit_lost` fence: writer open, recover, commit and the verifier record a
//! database below `store.watermark` as `harflow-fence-1\n<gen>\ncommit_lost\n`; probe and
//! readers never write a fence; an existing fence of either state is never rewritten.
mod common;
mod read_session_support;
mod verify_support;
mod watermark_support;
use hf_application::watermark::Slot;
use hf_storage::{Error, ReadRefusal, RootHold, Store};
use read_session_support::{later, open, permissions_bind, seeded};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    sync::Arc,
};
use verify_support::{FENCE, damaged, fence_bytes, open_store, verification_files};
use watermark_support::{
    Recorder, database_seq, file_bytes, file_of, generation, initialized, put, set_slot, write_file,
};

/// The best slot moved one past the database sequence.
fn ahead(root: &Path) {
    set_slot(root, Slot::B, database_seq(root) + 1);
}

fn fence_of(root: &Path) -> Vec<u8> {
    fs::read(root.join(FENCE)).unwrap()
}

/// Content and inode: a rewrite publishes a new file by rename, so the inode changes.
fn fence_stamp(root: &Path) -> (Vec<u8>, u64) {
    let inode = fs::symlink_metadata(root.join(FENCE)).unwrap().ino();
    (fence_of(root), inode)
}

fn recover(root: &Path) -> hf_storage::Result<hf_storage::Checkpoint> {
    Store::recover(root, later(10), common::lease::lease_port())
}

#[test]
fn commit_lost_at_writer_open_commit_and_verifier_writes_the_fence() {
    let first = seeded(3);
    let root = &first.root;
    let lost = fence_bytes(&generation(root), "commit_lost");
    ahead(root);
    assert!(matches!(open_store(root), Err(Error::CommitLost)));
    assert_eq!(fence_of(root), lost, "writer open");
    // The fence alone now refuses, whatever the watermark says.
    let refusal = open(root).err().as_ref().map(ReadRefusal::from);
    assert_eq!(refusal, Some(ReadRefusal::CommitLost));
    fs::remove_file(root.join(FENCE)).unwrap();
    assert!(matches!(recover(root), Err(Error::CommitLost)));
    assert_eq!(fence_of(root), lost, "recover");

    let (state, store) = initialized(Arc::new(Recorder::default()));
    let root = &state.root;
    put(&store, "one").unwrap();
    let generation = generation(root);
    write_file(root, &file_of(&generation, Some(5), Some(1)));
    assert!(matches!(put(&store, "two"), Err(Error::CommitLost)));
    assert_eq!(
        fence_of(root),
        fence_bytes(&generation, "commit_lost"),
        "commit"
    );
    assert_eq!(database_seq(root), 1, "nothing committed");

    let third = seeded(3);
    let root = &third.root;
    let store = open_store(root).unwrap();
    ahead(root);
    let verdict = store.verify_in_place(later(30));
    assert!(matches!(verdict, Err(Error::CommitLost)), "{verdict:?}");
    assert_eq!(
        fence_of(root),
        fence_bytes(&verify_support::generation(root), "commit_lost"),
        "verifier"
    );
}

/// With a `state` fence of this generation and the watermark ahead, every writer-role
/// path refuses as `want` and leaves the fence file as it was. Writer open and recover
/// run without a handle (recover needs the lease exclusively); commit and the verifier
/// run through a handle opened before the fence was written.
fn assert_fence_kept(state: &str, want: fn(&Error) -> bool) {
    let seeded = seeded(3);
    let root = &seeded.root;
    let watermark = file_bytes(root);
    fs::write(root.join(FENCE), fence_bytes(&generation(root), state)).unwrap();
    ahead(root);
    let before = fence_stamp(root);
    let opened = open_store(root).map(drop);
    assert!(
        opened.as_ref().is_err_and(want),
        "{state}: open: {opened:?}"
    );
    let recovered = recover(root).map(drop);
    assert!(
        recovered.as_ref().is_err_and(want),
        "{state}: recover: {recovered:?}"
    );
    assert_eq!(fence_stamp(root), before, "{state}: open and recover");

    fs::remove_file(root.join(FENCE)).unwrap();
    write_file(root, &watermark);
    let store = open_store(root).unwrap();
    fs::write(root.join(FENCE), fence_bytes(&generation(root), state)).unwrap();
    ahead(root);
    let before = fence_stamp(root);
    let committed = put(&store, "lost");
    assert!(
        committed.as_ref().is_err_and(want),
        "{state}: commit: {committed:?}"
    );
    let verdict = store.verify_in_place(later(30)).map(drop);
    assert!(
        verdict.as_ref().is_err_and(want),
        "{state}: verifier: {verdict:?}"
    );
    assert_eq!(fence_stamp(root), before, "{state}: commit and verifier");
}

#[test]
fn existing_failed_fence_is_not_overwritten() {
    assert_fence_kept("failed", |error| matches!(error, Error::Fenced));
}

#[test]
fn existing_commit_lost_fence_is_not_overwritten() {
    assert_fence_kept("commit_lost", |error| matches!(error, Error::CommitLost));

    // A store the verifier would fence `failed` keeps an existing `commit_lost` fence.
    let damaged = damaged();
    let root = &damaged.root;
    let store = open_store(root).unwrap();
    fs::write(
        root.join(FENCE),
        fence_bytes(&generation(root), "commit_lost"),
    )
    .unwrap();
    let before = fence_stamp(root);
    let verdict = store.verify_in_place(later(30));
    assert!(
        matches!(verdict, Err(Error::CommitLost)),
        "damaged: {verdict:?}"
    );
    assert_eq!(fence_stamp(root), before);
}

#[test]
fn probe_never_writes_a_fence() {
    let seeded = seeded(3);
    let root = &seeded.root;
    ahead(root);
    let watermark = file_bytes(root);
    let probed = hf_storage::probe(root, common::lease::lease_port());
    assert!(matches!(probed, Err(Error::CommitLost)), "{probed:?}");
    let session = open(root);
    assert!(
        matches!(session, Err(Error::CommitLost)),
        "{:?}",
        session.err()
    );
    // Root hold and the recovery read do not judge the watermark; they write nothing either.
    drop(RootHold::open(root, later(10), common::lease::lease_port()));
    assert!(
        verification_files(root).is_empty(),
        "{:?}",
        verification_files(root)
    );
    assert_eq!(
        file_bytes(root),
        watermark,
        "the watermark is left as it was"
    );
}

/// A fence that cannot be written leaves the refusal `CommitLost` and no fence.
#[test]
fn a_fence_write_failure_still_reports_commit_lost() {
    let seeded = seeded(3);
    let root = &seeded.root;
    if !permissions_bind(seeded.scratch.path()) {
        return;
    }
    ahead(root);
    fs::set_permissions(root, fs::Permissions::from_mode(0o555)).unwrap();
    let opened = open_store(root);
    fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(opened, Err(Error::CommitLost)),
        "{:?}",
        opened.err()
    );
    assert!(
        verification_files(root).is_empty(),
        "{:?}",
        verification_files(root)
    );
}

/// The verifier finds the database below the watermark while another holder keeps
/// `io.lock` past `RECORD_BUDGET`: the refusal is still `CommitLost` and no fence lands.
#[test]
fn a_record_lock_held_past_the_budget_reports_commit_lost_without_a_fence() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    ahead(root);
    let gate = fs::File::open(root.join("io.lock")).unwrap();
    gate.try_lock().unwrap();
    let started = std::time::Instant::now();
    let verdict = store.verify_in_place(later(30));
    let took = started.elapsed();
    drop(gate);
    assert!(matches!(verdict, Err(Error::CommitLost)), "{verdict:?}");
    assert!(
        took >= hf_storage::RECORD_BUDGET - std::time::Duration::from_millis(50),
        "{took:?}"
    );
    assert!(!root.join(FENCE).exists(), "no fence");
}

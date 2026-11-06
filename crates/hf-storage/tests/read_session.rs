#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Read session: correct rows from the real files, nothing written, no `io.lock`.
mod common;
mod read_session_support;
use common::default_limits;
use hf_application::lease::LeaseMode;
use hf_storage::{Error, ReadRefusal, ReadSession};
use read_session_support::{
    MAIN, SHM, WAL, bytes, checkpoint, expected, names, open, read_all, seeded,
};
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn reads_full_rows_from_a_store_whose_data_is_entirely_in_the_wal() {
    let seeded = seeded(40);
    // Baseline: the data really is only in the WAL.
    assert!(fs::metadata(seeded.root.join(WAL)).unwrap().len() > 4096);
    let before = (bytes(&seeded.root, MAIN), bytes(&seeded.root, WAL));
    let session = open(&seeded.root).unwrap();
    let rows = read_all(&session).unwrap();
    assert_eq!(rows.len(), 40);
    assert_eq!(rows, expected(seeded.rows.clone()));
    assert_eq!(
        (bytes(&seeded.root, MAIN), bytes(&seeded.root, WAL)),
        before
    );
}

#[test]
fn a_read_never_checkpoints_or_writes_main_or_wal() {
    let seeded = seeded(40);
    let before = (
        bytes(&seeded.root, MAIN),
        bytes(&seeded.root, WAL),
        fs::metadata(seeded.root.join(MAIN))
            .unwrap()
            .modified()
            .unwrap(),
    );
    for _ in 0..5 {
        let session = open(&seeded.root).unwrap();
        assert_eq!(read_all(&session).unwrap().len(), 40);
    }
    assert_eq!(bytes(&seeded.root, MAIN), before.0);
    assert_eq!(bytes(&seeded.root, WAL), before.1);
    assert_eq!(
        fs::metadata(seeded.root.join(MAIN))
            .unwrap()
            .modified()
            .unwrap(),
        before.2
    );
}

#[test]
fn a_checkpointed_store_reads_the_same_rows() {
    let seeded = seeded(40);
    checkpoint(&seeded.root);
    assert_eq!(fs::metadata(seeded.root.join(WAL)).unwrap().len(), 0);
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap(), expected(seeded.rows.clone()));
}

#[test]
fn a_missing_wal_is_refused_and_never_created() {
    let seeded = seeded(10);
    fs::remove_file(seeded.root.join(WAL)).unwrap();
    let error = open(&seeded.root).err().unwrap();
    assert!(matches!(error, Error::WalMissing), "{error:?}");
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::WalMissing);
    assert!(!seeded.root.join(WAL).exists());
}

#[test]
fn a_wal_removed_after_open_refuses_the_next_read_and_is_not_recreated() {
    let seeded = seeded(10);
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 10);
    fs::remove_file(seeded.root.join(WAL)).unwrap();
    let error = read_all(&session).err().unwrap();
    assert!(matches!(error, Error::WalMissing), "{error:?}");
    assert!(!seeded.root.join(WAL).exists());
}

#[test]
fn a_wal_that_is_not_a_regular_file_is_refused() {
    for kind in ["directory", "symlink"] {
        let seeded = seeded(10);
        let wal = seeded.root.join(WAL);
        fs::remove_file(&wal).unwrap();
        if kind == "directory" {
            fs::create_dir(&wal).unwrap();
        } else {
            let target = seeded.scratch.path().join("elsewhere");
            fs::write(&target, b"").unwrap();
            std::os::unix::fs::symlink(&target, &wal).unwrap();
        }
        let error = open(&seeded.root).err().unwrap();
        assert!(matches!(error, Error::WalMissing), "{kind}: {error:?}");
        assert!(fs::symlink_metadata(&wal).is_ok_and(|m| !m.is_file()));
    }
}

#[test]
fn an_absent_shm_beside_a_present_wal_reads_correctly() {
    let seeded = seeded(25);
    let _ = fs::remove_file(seeded.root.join(SHM));
    assert!(!seeded.root.join(SHM).exists());
    let wal_before = bytes(&seeded.root, WAL);
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap(), expected(seeded.rows.clone()));
    // A reader may create -shm; it never changes the WAL.
    assert_eq!(bytes(&seeded.root, WAL), wal_before);
}

#[test]
fn a_read_only_directory_is_a_typed_refusal_never_empty_results() {
    let seeded = seeded(10);
    if !read_session_support::permissions_bind(seeded.scratch.path()) {
        eprintln!("SKIPPED: directory permissions do not bind this user (superuser)");
        return;
    }
    let _ = fs::remove_file(seeded.root.join(SHM));
    fs::set_permissions(&seeded.root, fs::Permissions::from_mode(0o555)).unwrap();
    let outcome = open(&seeded.root).and_then(|session| read_all(&session));
    fs::set_permissions(&seeded.root, fs::Permissions::from_mode(0o755)).unwrap();
    let error = outcome.expect_err("a reader cannot create -shm in a read-only directory");
    assert!(
        ReadRefusal::from(&error)
            == ReadRefusal::Unavailable {
                file: "store.sqlite3"
            },
        "{error:?}"
    );
    assert!(!seeded.root.join(SHM).exists());
}

#[test]
fn io_lock_held_exclusively_does_not_block_a_read_and_no_scratch_appears() {
    let seeded = seeded(15);
    let parent = seeded.scratch.path().to_path_buf();
    let beside = names(&parent);
    let in_root = names(&seeded.root);
    let held = common::lease::lease_port()
        .acquire(
            &seeded.root.join("io.lock"),
            LeaseMode::Exclusive,
            false,
            read_session_support::later(5),
        )
        .unwrap();
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap(), expected(seeded.rows.clone()));
    drop(held);
    assert_eq!(names(&parent), beside);
    let created: Vec<_> = names(&seeded.root)
        .into_iter()
        .filter(|name| !in_root.contains(name))
        .collect();
    assert!(created.iter().all(|name| name == SHM), "{created:?}");
}

#[test]
fn the_session_takes_a_shared_store_lease_that_excludes_an_exclusive_holder() {
    let seeded = seeded(5);
    let session = open(&seeded.root).unwrap();
    let busy = common::lease::lease_port().acquire(
        &seeded.root.join("store.lock"),
        LeaseMode::Exclusive,
        false,
        read_session_support::later(1),
    );
    assert!(busy.is_err());
    drop(session);
    assert!(
        common::lease::lease_port()
            .acquire(
                &seeded.root.join("store.lock"),
                LeaseMode::Exclusive,
                false,
                read_session_support::later(1)
            )
            .is_ok()
    );
}

#[test]
fn a_session_on_a_missing_root_is_refused_without_creating_it() {
    let seeded = seeded(1);
    let missing = seeded.scratch.path().join("absent");
    let error = ReadSession::open(
        &missing,
        default_limits(),
        read_session_support::later(5),
        common::lease::lease_port(),
    )
    .err()
    .unwrap();
    assert_eq!(
        ReadRefusal::from(&error),
        ReadRefusal::Unavailable { file: "state root" }
    );
    assert!(!missing.exists());
}

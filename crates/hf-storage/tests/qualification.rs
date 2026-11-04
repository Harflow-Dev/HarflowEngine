#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits, snapshot_tree};
use hf_storage::{Error, Store, probe, probe_with_scratch};
use rusqlite::Connection;
use std::fs;

#[test]
fn missing_or_corrupt_protocol_marker_refuses_without_touching_files() {
    for marker in [None, Some(b"foreign protocol\n".as_slice())] {
        let fixture = Fixture::new();
        let root = fixture.path().join("store");
        drop(Store::initialize_fixture(&root, default_limits(), deadline()).unwrap());
        let path = root.join("store.identity");
        if let Some(bytes) = marker {
            fs::write(path, bytes).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        let before = snapshot_tree(&root);
        assert!(matches!(probe(&root), Err(Error::Incompatible)));
        assert!(matches!(
            Store::open(&root, default_limits(), deadline()),
            Err(Error::Incompatible)
        ));
        assert_eq!(snapshot_tree(&root), before);
    }
}

#[test]
fn well_formed_marker_generation_mismatch_refuses_all_paths_without_changes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    let mut different = store.identity().generation.clone();
    different.replace_range(0..1, if &different[0..1] == "0" { "1" } else { "0" });
    drop(store);
    fs::write(
        root.join("store.identity"),
        format!("harflow-storage-2\n1\n{different}\n"),
    )
    .unwrap();
    let before = snapshot_tree(&root);
    assert!(matches!(probe(&root), Err(Error::Incompatible)));
    assert_eq!(snapshot_tree(&root), before);
    assert!(matches!(
        Store::open(&root, default_limits(), deadline()),
        Err(Error::Incompatible)
    ));
    assert_eq!(snapshot_tree(&root), before);
    assert!(matches!(
        Store::recover(&root, deadline(), fixture.path()),
        Err(Error::Incompatible)
    ));
    assert_eq!(snapshot_tree(&root), before);
}

#[test]
fn missing_io_gate_is_an_old_protocol_refusal_without_changes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(Store::initialize_fixture(&root, default_limits(), deadline()).unwrap());
    fs::remove_file(root.join("io.lock")).unwrap();
    let before = snapshot_tree(&root);
    assert!(matches!(probe(&root), Err(Error::Incompatible)));
    assert_eq!(snapshot_tree(&root), before);
    assert!(matches!(
        Store::open(&root, default_limits(), deadline()),
        Err(Error::Incompatible)
    ));
    assert_eq!(snapshot_tree(&root), before);
}

#[test]
fn future_schema_present_only_in_committed_wal_is_rejected_without_source_changes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(Store::initialize_fixture(&root, default_limits(), deadline()).unwrap());
    let source = Connection::open(root.join("store.sqlite3")).unwrap();
    source
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    source.pragma_update(None, "user_version", 900_u32).unwrap();
    let wal = root.join("store.sqlite3-wal");
    assert!(wal.exists() && fs::metadata(&wal).unwrap().len() > 32);
    let before = snapshot_tree(&root);

    assert!(matches!(probe(&root), Err(Error::FutureSchema)));
    assert_eq!(snapshot_tree(&root), before);
    assert!(matches!(
        Store::open(&root, default_limits(), deadline()),
        Err(Error::FutureSchema)
    ));
    assert_eq!(snapshot_tree(&root), before);
    assert!(matches!(
        Store::recover(&root, deadline(), fixture.path()),
        Err(Error::FutureSchema)
    ));
    assert_eq!(snapshot_tree(&root), before);
    drop(source);
}

#[test]
fn expired_lock_handle_cannot_write_after_admission_deadline() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    let result = store.apply_batch(
        &[hf_storage::Mutation::Put {
            key: "late-lock".into(),
            value: b"no".to_vec(),
        }],
        std::time::Instant::now() - std::time::Duration::from_millis(1),
    );
    assert!(matches!(result, Err(Error::DeadlineExceeded)));
    assert!(
        store
            .read_batch(&["late-lock".into()], deadline())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn scratch_parent_inside_source_root_is_rejected_without_source_changes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(Store::initialize_fixture(&root, default_limits(), deadline()).unwrap());
    let child = root.join("scratch");
    fs::create_dir(&child).unwrap();
    for scratch in [&root, &child, &child.join("..")] {
        let before = snapshot_tree(&root);
        assert!(matches!(
            probe_with_scratch(&root, deadline(), scratch),
            Err(Error::InvalidRoot)
        ));
        assert_eq!(snapshot_tree(&root), before);
    }
}

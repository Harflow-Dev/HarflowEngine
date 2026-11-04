#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::{Error, Mutation, Store};
use rusqlite::Connection;

#[test]
fn passive_checkpoint_reports_long_reader_incomplete_then_completes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .apply_batch(
            &[Mutation::Put {
                key: "value".into(),
                value: b"old".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    let reader = Connection::open(root.join("store.sqlite3")).unwrap();
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let old: Vec<u8> = reader
        .query_row(
            "SELECT value FROM fixture_records WHERE key='value'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old, b"old");
    store
        .apply_batch(
            &[Mutation::Put {
                key: "value".into(),
                value: b"new".to_vec(),
            }],
            deadline(),
        )
        .unwrap();

    let incomplete = store.checkpoint_passive(deadline()).unwrap();
    assert!(!incomplete.complete);
    assert!(incomplete.log_frames > incomplete.checkpointed_frames);
    let still_old: Vec<u8> = reader
        .query_row(
            "SELECT value FROM fixture_records WHERE key='value'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_old, b"old");
    reader.execute_batch("COMMIT").unwrap();
    let complete = store.checkpoint_passive(deadline()).unwrap();
    assert!(complete.complete);
    assert_eq!(complete.log_frames, complete.checkpointed_frames);
}

#[test]
fn explicit_recovery_refuses_while_shared_handles_live_then_truncates_after_close() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .apply_batch(
            &[Mutation::Put {
                key: "persist".into(),
                value: b"yes".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    assert!(matches!(
        Store::recover(&root, deadline(), fixture.path()),
        Err(Error::Busy)
    ));
    assert_eq!(
        store.read_batch(&["persist".into()], deadline()).unwrap()[0].value,
        b"yes"
    );
    drop(store);

    let recovered = Store::recover(&root, deadline(), fixture.path()).unwrap();
    assert!(recovered.complete);
    assert_eq!(recovered.busy, 0);
    let reopened = Store::open(&root, default_limits(), deadline()).unwrap();
    assert_eq!(
        reopened
            .read_batch(&["persist".into()], deadline())
            .unwrap()[0]
            .value,
        b"yes"
    );
}

#[test]
fn exclusive_io_gate_blocks_store_and_probe_without_late_effects() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    {
        let gate = std::fs::File::open(root.join("io.lock")).unwrap();
        gate.try_lock().unwrap();
        assert!(matches!(
            store.apply_batch(
                &[Mutation::Put {
                    key: "blocked".into(),
                    value: b"no".to_vec()
                }],
                deadline()
            ),
            Err(Error::Busy)
        ));
        assert!(matches!(
            store.checkpoint_passive(deadline()),
            Err(Error::Busy)
        ));
        assert!(matches!(hf_storage::probe(&root), Err(Error::Busy)));
        assert!(matches!(
            Store::open(&root, default_limits(), deadline()),
            Err(Error::Busy)
        ));
    }
    assert!(
        store
            .read_batch(&["blocked".into()], deadline())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn dropping_final_store_handle_preserves_main_and_wal_bytes() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .apply_batch(
            &[Mutation::Put {
                key: "durable".into(),
                value: b"wal-bytes".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    let main = std::fs::read(root.join("store.sqlite3")).unwrap();
    let wal_path = root.join("store.sqlite3-wal");
    let wal = std::fs::read(&wal_path).unwrap();
    assert!(!wal.is_empty());

    drop(store);
    assert_eq!(std::fs::read(root.join("store.sqlite3")).unwrap(), main);
    assert_eq!(std::fs::read(&wal_path).unwrap(), wal);
}

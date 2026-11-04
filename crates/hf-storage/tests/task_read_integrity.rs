#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{Error, Limits, Store, TaskFixture};
use rusqlite::Connection;

fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

fn row(value: &str, stamp: i64) -> TaskRow {
    TaskRow {
        id: id(value),
        stamp,
        session: None,
        observation: StoredObservation {
            recorded_status: None,
            active: None,
            exited: None,
            drained: None,
            owned: None,
        },
        header: Vec::new(),
        status: None,
    }
}

#[test]
fn non_boolean_sqlite_flags_never_become_true_facts() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row("flagged-task", 1)],
                pending: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline(),
        )
        .unwrap();
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE task_headers SET active=2 WHERE id='flagged-task';").unwrap();
    let active_only = TaskQuery::page(10, PageFilter::new(true, None).unwrap(), None).unwrap();

    let excluded = store
        .task_read(&AuthorizedScope::Global, &active_only, deadline())
        .unwrap();
    assert!(excluded.rows.is_empty());
    assert!(!excluded.activity_complete);
    assert_eq!(excluded.count, None);
    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(vec![id("flagged-task")]).unwrap(),
            deadline()
        ),
        Err(Error::Sqlite(rusqlite::Error::InvalidQuery))
    ));

    raw.execute_batch("UPDATE task_headers SET active=1 WHERE id='flagged-task'; UPDATE task_dataset SET discovery=2 WHERE singleton=1;").unwrap();
    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(Vec::new()).unwrap(),
            deadline()
        ),
        Err(Error::Incompatible)
    ));
}

#[test]
fn oversized_persisted_generation_and_ids_are_bounded_before_mapping() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let limits = Limits {
        max_rows: 1,
        max_bytes: 256,
    };
    let store = Store::initialize_fixture(&root, limits, deadline()).unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row("prefix-123456-item", 1)],
                pending: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline(),
        )
        .unwrap();
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    let generation: String = raw
        .query_row(
            "SELECT generation FROM task_dataset WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    let oversized_generation = "g".repeat(100_000);
    raw.execute(
        "UPDATE task_dataset SET generation=?1 WHERE singleton=1",
        [&oversized_generation],
    )
    .unwrap();
    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(Vec::new()).unwrap(),
            deadline()
        ),
        Err(Error::TaskReadUnavailable)
    ));
    let embedded_nul = format!("{}\0{}", "a".repeat(32), "z".repeat(10_000));
    raw.execute(
        "UPDATE task_dataset SET generation=?1 WHERE singleton=1",
        [&embedded_nul],
    )
    .unwrap();
    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(Vec::new()).unwrap(),
            deadline()
        ),
        Err(Error::TaskReadUnavailable)
    ));

    raw.execute(
        "UPDATE task_dataset SET generation=?1 WHERE singleton=1",
        [&generation],
    )
    .unwrap();
    let oversized_id = format!("prefix-123456{}", "x".repeat(100_000));
    raw.execute(
        "UPDATE task_headers SET id=?1 WHERE id='prefix-123456-item'",
        [&oversized_id],
    )
    .unwrap();
    let prefix = TaskQuery::ids(vec![id("prefix-123456")]).unwrap();
    assert!(matches!(
        store.task_read(&AuthorizedScope::Global, &prefix, deadline()),
        Err(Error::Incompatible)
    ));
}

#[test]
fn fixture_replacement_refuses_future_dataset_version_without_erasing_rows() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row("preserve-this", 1)],
                pending: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline(),
        )
        .unwrap();
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    let generation: String = raw
        .query_row(
            "SELECT generation FROM task_dataset WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE task_dataset SET version=2;")
        .unwrap();

    assert!(matches!(
        store.replace_task_fixture(
            &TaskFixture {
                rows: vec![row("replacement", 2)],
                pending: Vec::new(),
                coverage: Coverage {
                    discovery_complete: false,
                    ownership_complete: false,
                    source_complete: false
                }
            },
            deadline()
        ),
        Err(Error::TaskReadUnavailable)
    ));
    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(Vec::new()).unwrap(),
            deadline()
        ),
        Err(Error::TaskReadUnavailable)
    ));
    let count: i64 = raw
        .query_row(
            "SELECT count(*) FROM task_headers WHERE id='preserve-this'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let stored_generation: String = raw
        .query_row(
            "SELECT generation FROM task_dataset WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(stored_generation, generation);
}

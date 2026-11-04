#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, Lookup, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{Error, Store, TaskFixture};
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
fn exact_short_id_and_eight_character_prefix_follow_distinct_rules() {
    let fixture = Fixture::new();
    let store =
        Store::initialize_fixture(fixture.path().join("store"), default_limits(), deadline())
            .unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row("tiny", 2), row("task1234-tail", 1)],
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
    let query = TaskQuery::ids(vec![id("tiny"), id("task1234"), id("task123")]).unwrap();

    let result = store
        .task_read(&AuthorizedScope::Global, &query, deadline())
        .unwrap();

    assert_eq!(
        result.lookups,
        [
            Lookup::Found(id("tiny")),
            Lookup::Found(id("task1234-tail")),
            Lookup::InvalidPrefix
        ]
    );
    assert_eq!(
        result
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["tiny", "task1234-tail"]
    );
}

#[test]
fn equal_stamp_keyset_pages_visit_every_id_once_in_descending_order() {
    let fixture = Fixture::new();
    let store =
        Store::initialize_fixture(fixture.path().join("store"), default_limits(), deadline())
            .unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![
                    row("sort-c", 50),
                    row("sort-a", 50),
                    row("sort-f", 50),
                    row("sort-b", 50),
                    row("sort-e", 50),
                    row("sort-d", 50),
                ],
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
    let expected = ["sort-f", "sort-e", "sort-d", "sort-c", "sort-b", "sort-a"];
    let filter = PageFilter::new(false, None).unwrap();
    let mut cursor = None;
    let mut observed = Vec::new();

    for _ in 0..expected.len() {
        let query = TaskQuery::page(1, filter.clone(), cursor).unwrap();
        let page = store
            .task_read(&AuthorizedScope::Global, &query, deadline())
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        observed.push(page.rows[0].id.as_str().to_owned());
        cursor = page.cursor;
    }

    assert_eq!(observed, expected);
    assert!(cursor.is_none());
}

#[test]
fn explicit_id_reads_keep_input_order_canonical_exactness_and_absence() {
    let fixture = Fixture::new();
    let store =
        Store::initialize_fixture(fixture.path().join("store"), default_limits(), deadline())
            .unwrap();
    let canonical = "0123456789abcdef0123456789abcdef";
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row("id-a-1234", 1), row("id-b-1234", 2), row(canonical, 3)],
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
    let query = TaskQuery::ids(vec![
        id("id-a-1234"),
        id("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        id("id-b-1234"),
        id("id-a-1234"),
        id(canonical),
    ])
    .unwrap();

    let result = store
        .task_read(&AuthorizedScope::Global, &query, deadline())
        .unwrap();

    assert_eq!(
        result.lookups,
        [
            Lookup::Found(id("id-a-1234")),
            Lookup::Missing,
            Lookup::Found(id("id-b-1234")),
            Lookup::Found(id(canonical))
        ]
    );
    assert_eq!(
        result
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["id-a-1234", "id-b-1234", canonical]
    );
    assert_eq!(result.count, None);
    let status = store
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::StoredStatus(id(canonical)),
            deadline(),
        )
        .unwrap();
    assert_eq!(status.rows[0].id.as_str(), canonical);
    assert_eq!(status.rows[0].status, None);
}

#[test]
fn preexisting_root_without_task_tables_is_unavailable_and_never_upgraded() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    drop(store);
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    raw.execute_batch("DROP TABLE task_pending; DROP TABLE task_headers; DROP TABLE task_dataset;")
        .unwrap();
    drop(raw);
    let store = Store::open(&root, default_limits(), deadline()).unwrap();

    assert!(matches!(
        store.task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(Vec::new()).unwrap(),
            deadline()
        ),
        Err(Error::TaskReadUnavailable)
    ));
    assert!(matches!(
        store.replace_task_fixture(
            &TaskFixture {
                rows: Vec::new(),
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
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    let remaining: i64 = raw.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('task_dataset','task_headers','task_pending')", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 0);
}

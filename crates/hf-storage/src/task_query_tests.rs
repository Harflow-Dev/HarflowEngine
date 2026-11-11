#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Limits, Store, TaskFixture, task_page::page_sql, task_read::DATASET_QUERY};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, Cursor, PageFilter, SortKey, StoredObservation, TaskId,
};
use rusqlite::{Connection, params_from_iter};
use std::{
    fs,
    time::{Duration, Instant},
};

fn explain(connection: &Connection, filter: &PageFilter, cursor: Option<&Cursor>) -> Vec<String> {
    let (sql, params) = page_sql(&AuthorizedScope::Global, filter, cursor, 10);
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    statement
        .query_map(params_from_iter(params.iter()), |row| row.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn generated_page_queries_seek_the_matching_index_without_sorting() {
    let _guard = crate::tests::fixture_guard();
    let parent = std::env::temp_dir().join(format!("hf-query-plan-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let connection = Connection::open(root.join("store.sqlite3")).unwrap();
    for (filter, index) in [
        (PageFilter::new(false, None).unwrap(), "task_order"),
        (PageFilter::new(true, None).unwrap(), "task_active"),
        (
            PageFilter::new(false, Some("session-a".into())).unwrap(),
            "task_session",
        ),
    ] {
        let plan = explain(&connection, &filter, None).join(" ");
        assert!(
            plan.contains(index),
            "expected {index} in query plan: {plan}"
        );
        assert!(
            !plan.contains("USE TEMP B-TREE"),
            "query plan sorts instead of seeking: {plan}"
        );
    }
    let filter = PageFilter::new(false, None).unwrap();
    let cursor = Cursor {
        store: "a".repeat(32),
        dataset: "b".repeat(32),
        scope: AuthorizedScope::Global,
        filter: filter.clone(),
        after: SortKey {
            stamp: 10,
            id: TaskId::new("cursor-id".into()).unwrap(),
        },
    };
    let plan = explain(&connection, &filter, Some(&cursor)).join(" ");
    assert!(
        plan.contains("task_order"),
        "keyset query missed order index: {plan}"
    );
    assert!(
        plan.contains("SEARCH"),
        "keyset query did not seek its range: {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE"),
        "keyset query sorts instead of seeking: {plan}"
    );
    drop(connection);
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn production_selects_return_only_their_bounded_generation_and_identifier_prefixes() {
    let _guard = crate::tests::fixture_guard();
    let parent = std::env::temp_dir().join(format!("hf-query-projection-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits {
            max_rows: 2,
            max_bytes: 128,
        },
        Instant::now() + Duration::from_secs(2),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![TaskRow {
                    id: TaskId::new("small-row".into()).unwrap(),
                    stamp: 1,
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
                }],
                launches: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    let connection = Connection::open(root.join("store.sqlite3")).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    let long_generation = "g".repeat(100_000);
    connection
        .execute("UPDATE task_dataset SET generation=?1", [&long_generation])
        .unwrap();
    let projected_generation: String = connection
        .query_row(DATASET_QUERY, [], |row| row.get(1))
        .unwrap();
    assert_eq!(projected_generation.len(), 33);

    let long_id = format!("small-row{}", "x".repeat(100_000));
    connection
        .execute(
            "UPDATE task_headers SET id=?1 WHERE id='small-row'",
            [&long_id],
        )
        .unwrap();
    let filter = PageFilter::new(false, None).unwrap();
    let (sql, parameters) = page_sql(&AuthorizedScope::Global, &filter, None, 1);
    let projected_id: String = connection
        .query_row(&sql, params_from_iter(parameters.iter()), |row| row.get(0))
        .unwrap();
    assert_eq!(projected_id.len(), 65);

    drop(connection);
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, Lookup, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{Error, Store, TaskFixture};
use std::time::Instant;

fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

fn row(value: &str, stamp: i64, active: Option<bool>, session: Option<&str>) -> TaskRow {
    TaskRow {
        id: id(value),
        stamp,
        session: session.map(str::to_owned),
        observation: StoredObservation {
            recorded_status: Some("supplied-status".into()),
            active,
            exited: None,
            drained: Some(false),
            owned: None,
        },
        header: format!("header:{value}").into_bytes(),
        status: Some(b"opaque status bytes".to_vec()),
    }
}

fn seed(store: &Store, rows: Vec<TaskRow>, pending: &[&str], coverage: Coverage) {
    store
        .replace_task_fixture(
            &TaskFixture {
                rows,
                pending: pending.iter().map(|value| id(value)).collect(),
                coverage,
            },
            deadline(),
        )
        .unwrap();
}

fn complete() -> Coverage {
    Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    }
}

fn page(limit: usize, active_only: bool, session: Option<&str>) -> TaskQuery {
    TaskQuery::page(
        limit,
        PageFilter::new(active_only, session.map(str::to_owned)).unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn fixture_payload_bounds_reject_oversized_fields_and_page_stops_at_byte_budget() {
    let fixture = Fixture::new();
    let store = Store::initialize_fixture(
        fixture.path().join("store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let mut oversized = row("oversized", 0, Some(true), None);
    oversized.header = vec![b'h'; 2049];
    assert!(matches!(
        store.replace_task_fixture(
            &TaskFixture {
                rows: vec![oversized],
                pending: Vec::new(),
                coverage: complete()
            },
            deadline()
        ),
        Err(Error::LimitExceeded)
    ));
    let mut oversized_status = row("oversized-status", 0, Some(true), None);
    oversized_status.status = Some(vec![b's'; 2049]);
    assert!(matches!(
        store.replace_task_fixture(
            &TaskFixture {
                rows: vec![oversized_status],
                pending: Vec::new(),
                coverage: complete()
            },
            deadline()
        ),
        Err(Error::LimitExceeded)
    ));

    let large_rows = (0..80)
        .map(|index| {
            let mut item = row(&format!("large-{index:03}"), 80 - index, Some(true), None);
            item.header = vec![b'h'; 2048];
            item.status = Some(vec![b's'; 2048]);
            item
        })
        .collect();
    seed(&store, large_rows, &[], complete());
    let result = store
        .task_read(
            &AuthorizedScope::Global,
            &page(100, false, None),
            deadline(),
        )
        .unwrap();

    assert!(result.partial);
    assert!(!result.rows.is_empty());
    assert!(result.rows.len() < 80);
    assert!(result.cursor.is_some());
}

#[test]
fn incomplete_coverage_keeps_prefix_and_count_unknown() {
    let fixture = Fixture::new();
    let store = Store::initialize_fixture(
        fixture.path().join("store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    seed(
        &store,
        vec![row("task-prefix-1", 1, Some(false), None)],
        &[],
        Coverage {
            discovery_complete: true,
            ownership_complete: false,
            source_complete: true,
        },
    );

    let listed = store
        .task_read(&AuthorizedScope::Global, &page(10, false, None), deadline())
        .unwrap();
    let lookup = store
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(vec![id("task-prefix")]).unwrap(),
            deadline(),
        )
        .unwrap();

    assert_eq!(listed.count, None);
    assert_eq!(lookup.lookups, [Lookup::Incomplete]);
}

#[test]
fn page_cursor_binds_store_dataset_scope_and_filter_and_replacement_invalidates_it() {
    let fixture = Fixture::new();
    let store = Store::initialize_fixture(
        fixture.path().join("store"),
        hf_storage::Limits {
            max_rows: 128,
            max_bytes: 4096,
        },
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let rows = vec![
        row("page-a-123", 3, Some(true), None),
        row("page-b-123", 2, Some(true), None),
        row("page-c-123", 1, Some(true), None),
    ];
    seed(&store, rows.clone(), &[], complete());
    let scope = AuthorizedScope::Global;
    let first = store
        .task_read(&scope, &page(1, false, None), deadline())
        .unwrap();
    let cursor = first
        .cursor
        .clone()
        .expect("bounded first page should expose continuation");
    let next = TaskQuery::page(
        1,
        PageFilter::new(false, None).unwrap(),
        Some(cursor.clone()),
    )
    .unwrap();
    let second = store.task_read(&scope, &next, deadline()).unwrap();
    assert_eq!(first.rows[0].id.as_str(), "page-a-123");
    assert_eq!(second.rows[0].id.as_str(), "page-b-123");

    let wrong_filter = TaskQuery::page(
        1,
        PageFilter::new(true, None).unwrap(),
        Some(cursor.clone()),
    )
    .unwrap();
    assert!(matches!(
        store.task_read(&scope, &wrong_filter, deadline()),
        Err(Error::StaleTaskCursor)
    ));
    let wrong_scope = AuthorizedScope::Closed(vec![id("page-a-123")]);
    assert!(matches!(
        store.task_read(&wrong_scope, &next, deadline()),
        Err(Error::StaleTaskCursor)
    ));
    let other = Store::initialize_fixture(
        fixture.path().join("other-store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    seed(&other, rows.clone(), &[], complete());
    assert!(matches!(
        other.task_read(&scope, &next, deadline()),
        Err(Error::StaleTaskCursor)
    ));
    seed(&store, rows, &[], complete());
    assert!(matches!(
        store.task_read(&scope, &next, deadline()),
        Err(Error::StaleTaskCursor)
    ));
}

#[test]
fn expired_read_has_no_late_fixture_mutation() {
    let fixture = Fixture::new();
    let store = Store::initialize_fixture(
        fixture.path().join("store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    seed(
        &store,
        vec![row("stable-row", 1, Some(true), None)],
        &[],
        complete(),
    );
    let expired = Instant::now();

    assert!(matches!(
        store.task_read(&AuthorizedScope::Global, &page(10, false, None), expired),
        Err(Error::DeadlineExceeded)
    ));
    let result = store
        .task_read(&AuthorizedScope::Global, &page(10, false, None), deadline())
        .unwrap();
    assert_eq!(
        result
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["stable-row"]
    );
}

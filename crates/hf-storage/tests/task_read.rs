#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, Lookup, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{Store, TaskFixture};

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
fn indexed_page_orders_equal_stamps_by_id_and_preserves_independent_observations() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    seed(
        &store,
        vec![
            row("equal-b", 9, Some(true), None),
            row("older-z", 8, Some(false), None),
            row("equal-a", 9, Some(true), None),
        ],
        &[],
        complete(),
    );

    let result = store
        .task_read(&AuthorizedScope::Global, &page(10, false, None), deadline())
        .unwrap();

    assert_eq!(
        result
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["equal-b", "equal-a", "older-z"]
    );
    assert_eq!(
        result.rows[0].observation.recorded_status.as_deref(),
        Some("supplied-status")
    );
    assert_eq!(result.rows[0].observation.active, Some(true));
    assert_eq!(result.rows[0].observation.exited, None);
    assert_eq!(result.rows[0].observation.drained, Some(false));
    assert_eq!(result.rows[0].observation.owned, None);
    assert_eq!(result.count, Some(3));
}

#[test]
fn filters_apply_only_supplied_active_and_session_facts() {
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
        vec![
            row("s-active", 4, Some(true), Some("s")),
            row("s-inactive", 3, Some(false), Some("s")),
            row("s-unknown", 2, None, Some("s")),
            row("other-active", 1, Some(true), Some("other")),
        ],
        &[],
        complete(),
    );

    let active = store
        .task_read(
            &AuthorizedScope::Global,
            &page(10, true, Some("s")),
            deadline(),
        )
        .unwrap();
    let session = store
        .task_read(
            &AuthorizedScope::Global,
            &page(10, false, Some("s")),
            deadline(),
        )
        .unwrap();

    assert_eq!(
        active
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["s-active"]
    );
    assert!(!active.activity_complete);
    assert_eq!(
        session
            .rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["s-active", "s-inactive", "s-unknown"]
    );
    assert_eq!(session.rows[2].observation.active, None);
    assert!(!session.activity_complete);
    assert_eq!(session.count, None);
}

#[test]
fn closed_namespace_hides_absent_and_unauthorized_prefix_candidates() {
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
        vec![
            row("private-123456-a", 1, Some(true), None),
            row("private-123456-b", 1, Some(true), None),
        ],
        &[],
        complete(),
    );
    let scope = AuthorizedScope::Closed(vec![id("allowed-123456")]);
    let query = TaskQuery::ids(vec![id("private-123456"), id("missing-123456")]).unwrap();

    let result = store.task_read(&scope, &query, deadline()).unwrap();

    assert_eq!(result.lookups, [Lookup::Missing, Lookup::Missing]);
    assert!(result.rows.is_empty());
    assert_eq!(result.count, None);
}

#[test]
fn exact_pending_lookup_is_separate_and_prefix_resolution_caps_ambiguity() {
    let fixture = Fixture::new();
    let store = Store::initialize_fixture(
        fixture.path().join("store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let rows = (0..6)
        .map(|n| row(&format!("task-prefix-{n}"), n, Some(false), None))
        .collect();
    seed(&store, rows, &["pending-123456"], complete());
    let query = TaskQuery::ids(vec![
        id("pending-123456"),
        id("pending-123"),
        id("task-prefix-"),
        id("0123456789abcdef0123456789abcdef"),
        id("task"),
    ])
    .unwrap();

    let result = store
        .task_read(&AuthorizedScope::Global, &query, deadline())
        .unwrap();

    assert_eq!(result.lookups[0], Lookup::Pending(id("pending-123456")));
    assert_eq!(result.lookups[1], Lookup::Missing);
    assert_eq!(
        result.lookups[2],
        Lookup::Ambiguous {
            candidates: vec![
                id("task-prefix-0"),
                id("task-prefix-1"),
                id("task-prefix-2"),
                id("task-prefix-3"),
                id("task-prefix-4")
            ],
            truncated: true
        }
    );
    assert_eq!(result.lookups[3], Lookup::Missing);
    assert_eq!(result.lookups[4], Lookup::InvalidPrefix);
}

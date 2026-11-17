use super::support::*;
use crate::{
    fixture_support::{A, later, world},
    task_writer_support::running,
};
use hf_domain::task_lifecycle::ExecutionObservation;
use hf_storage::Error;

#[test]
fn preparation_publishes_reservation_task_and_launch_association_together() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 1);
    let evidence = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    assert_eq!(evidence.revision, 0);
    assert_eq!(evidence.binding.task.as_str(), A);
    assert_eq!(evidence.ownership.request.operation, request.operation);
    assert_eq!(evidence.observation, ExecutionObservation::default());
    assert_eq!(
        world
            .store
            .read_lifecycle(&evidence.binding, later(5))
            .unwrap(),
        Some(evidence)
    );

    let db = world.db();
    let rows: i64 = db
        .query_row("SELECT count(*) FROM task_facts WHERE id=?1", [A], |r| {
            r.get(0)
        })
        .unwrap();
    let launches: i64 = db
        .query_row(
            "SELECT count(*) FROM launch_attempts WHERE name=?1",
            [A],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((rows, launches), (1, 1));
}

#[test]
fn a_late_preparation_write_failure_rolls_back_reservation_task_and_launch_rows() {
    let world = world();
    world
        .raw()
        .execute_batch(
            "CREATE TRIGGER reject_lifecycle BEFORE INSERT ON lifecycle_records
         BEGIN SELECT RAISE(ABORT, 'injected lifecycle write failure'); END;",
        )
        .unwrap();
    let task = running(A);
    let request = request(&world.store, 5);
    let before: i64 = world
        .db()
        .query_row(
            "SELECT value FROM commit_sequence WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let error = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::Sqlite(_)));
    let db = world.db();
    for table in [
        "ownership_records",
        "task_facts",
        "task_headers",
        "task_prompts",
        "launch_attempts",
        "lifecycle_records",
    ] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table} escaped the preparation transaction");
    }
    let after: i64 = db
        .query_row(
            "SELECT value FROM commit_sequence WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn preparation_rejects_terminal_facts_before_any_public_task_row_is_written() {
    let world = world();
    let mut task = running(A);
    task.facts.status = "completed".into();
    let request = request(&world.store, 6);
    let error = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    let db = world.db();
    for table in [
        "ownership_records",
        "task_facts",
        "task_headers",
        "task_prompts",
        "launch_attempts",
        "lifecycle_records",
    ] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "terminal preparation exposed a row in {table}");
    }
}

#[test]
fn preparation_refuses_an_unvalidated_public_projection_without_writing() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 9);
    let error = world
        .store
        .prepare_lifecycle(&request, &task, &REJECT, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    let db = world.db();
    for table in [
        "ownership_records",
        "task_facts",
        "launch_attempts",
        "lifecycle_records",
    ] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "rejected projection wrote {table}");
    }
}

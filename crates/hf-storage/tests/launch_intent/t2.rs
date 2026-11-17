use super::support::*;
use crate::fixture_support::{self, A, B, one, world};
use hf_storage::Error;

fn public_row_counts(world: &crate::fixture_support::World, task: &str) -> [i64; 6] {
    let db = world.db();
    [
        crate::fixture_support::count(&db, "task_facts", "id", task),
        crate::fixture_support::count(&db, "task_headers", "id", task),
        crate::fixture_support::count(&db, "task_prompts", "task", task),
        crate::fixture_support::count(&db, "task_processes", "task", task),
        crate::fixture_support::count(&db, "event_streams", "task", task),
        crate::fixture_support::count(&db, "event_records", "task", task),
    ]
}

#[test]
fn t2_publishes_the_running_task_and_actual_child_together() {
    let world = world();
    let request = request(&world.store, 70);
    let intent = create(A, "Read and summarize this project.");
    let intent_gate = IntentGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let initial = world
        .store
        .prepare_launch_intent(&request, &intent, &intent_gate, later(5))
        .unwrap();
    assert_eq!(public_row_counts(&world, A), [0; 6]);
    let (revision, fence) = claim(&world.store, &initial.binding);
    let watch_before_spawn = world.watch();
    let actual_child = child();
    let task = task_for_spawn(A, &intent, &actual_child);
    let spawn_gate = SpawnGate::new(false);

    let published = world
        .store
        .publish_launch_spawn(
            &initial.binding,
            &fence,
            revision,
            &spawn_publication(&task, &actual_child, &spawn_gate),
            later(5),
        )
        .unwrap();

    assert_eq!(published.revision, revision + 1);
    assert_eq!(published.ownership.child, Some(actual_child.clone()));
    assert_eq!(
        published.ownership.phase,
        hf_domain::ownership::Phase::SpawnObserved
    );
    assert_eq!(world.watch(), watch_before_spawn + 1);
    assert_eq!(public_row_counts(&world, A), [1, 1, 1, 1, 0, 0]);
    assert_eq!(spawn_gate.calls.get(), 1);
    assert_eq!(
        spawn_gate.observed.borrow().as_slice(),
        &[SpawnObservation {
            binding: initial.binding.clone(),
            task: fixture_support::tid(A),
            child: actual_child.clone(),
            prompt: intent.prompt.clone(),
        }]
    );
    assert_eq!(
        one::<String>(
            &world.db(),
            "SELECT state FROM launch_attempts WHERE name=?1",
            [A]
        ),
        "spawned"
    );
    assert_eq!(
        one::<i64>(
            &world.db(),
            "SELECT pid FROM task_processes WHERE task=?1",
            [A]
        ),
        i64::from(actual_child.pid())
    );
}

#[test]
fn t2_refuses_a_child_that_does_not_match_the_task_process_identity() {
    let world = world();
    let request = request(&world.store, 71);
    let intent = create(A, "Summarize.");
    let initial = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &initial.binding);
    let actual_child = child();
    let mut task = task_for_spawn(A, &intent, &actual_child);
    task.process = Some(hf_storage::TaskProcess {
        pid: actual_child.pid() + 1,
        start: Some(actual_child.clone()),
    });
    let spawn_gate = SpawnGate::new(false);
    let watch_before = world.watch();

    let error = world
        .store
        .publish_launch_spawn(
            &initial.binding,
            &fence,
            revision,
            &spawn_publication(&task, &actual_child, &spawn_gate),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    assert_eq!(spawn_gate.calls.get(), 0);
    assert_eq!(public_row_counts(&world, A), [0; 6]);
    assert_eq!(world.watch(), watch_before);
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, revision);
    assert_eq!(after.ownership.child, None);
}

#[test]
fn refused_spawn_projection_leaves_t0_intent_and_claim_intact() {
    let world = world();
    let request = request(&world.store, 72);
    let intent = create(B, "Summarize.");
    let initial = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &initial.binding);
    let actual_child = child();
    let task = task_for_spawn(B, &intent, &actual_child);
    let spawn_gate = SpawnGate::new(true);
    let watch_before = world.watch();

    let error = world
        .store
        .publish_launch_spawn(
            &initial.binding,
            &fence,
            revision,
            &spawn_publication(&task, &actual_child, &spawn_gate),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    assert_eq!(spawn_gate.calls.get(), 1);
    assert_eq!(public_row_counts(&world, B), [0; 6]);
    assert_eq!(world.watch(), watch_before);
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, revision);
    assert_eq!(after.ownership.phase, hf_domain::ownership::Phase::Claimed);
    assert_eq!(after.ownership.child, None);
    assert_eq!(
        world
            .store
            .read_launch_intent(&initial.binding, later(5))
            .unwrap()
            .unwrap()
            .create,
        intent
    );
}

#[test]
fn t2_cannot_publish_terminal_status_or_finished_facts() {
    let world = world();
    let request = request(&world.store, 74);
    let intent = create(B, "Summarize.");
    let initial = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &initial.binding);
    let actual_child = child();
    let mut task = task_for_spawn(B, &intent, &actual_child);
    task.facts.status = "completed".into();
    task.facts.exit_code = Some(0);
    task.facts.finished_at = Some("2026-01-10T10:05:00+00:00".into());
    task.facts.finished_at_us = Some(1_768_039_500_000_000);
    task.facts.output_drained = true;
    let spawn_gate = SpawnGate::new(false);
    let watch_before = world.watch();

    let error = world
        .store
        .publish_launch_spawn(
            &initial.binding,
            &fence,
            revision,
            &spawn_publication(&task, &actual_child, &spawn_gate),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    assert_eq!(spawn_gate.calls.get(), 0);
    assert_eq!(public_row_counts(&world, B), [0; 6]);
    assert_eq!(world.watch(), watch_before);
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, revision);
    assert_eq!(after.ownership.child, None);
}

#[test]
fn late_t2_storage_failure_rolls_back_task_rows_process_claim_and_watch() {
    let world = world();
    let request = request(&world.store, 73);
    let intent = create(A, "Summarize.");
    let initial = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &initial.binding);
    world
        .raw()
        .execute_batch(
            "CREATE TRIGGER reject_spawn_update BEFORE UPDATE ON lifecycle_records
             BEGIN SELECT RAISE(ABORT, 'injected T2 update failure'); END;",
        )
        .unwrap();
    let actual_child = child();
    let task = task_for_spawn(A, &intent, &actual_child);
    let spawn_gate = SpawnGate::new(false);
    let watch_before = world.watch();

    let error = world
        .store
        .publish_launch_spawn(
            &initial.binding,
            &fence,
            revision,
            &spawn_publication(&task, &actual_child, &spawn_gate),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Sqlite(_)));
    assert_eq!(public_row_counts(&world, A), [0; 6]);
    assert_eq!(world.watch(), watch_before);
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, revision);
    assert_eq!(after.ownership.phase, hf_domain::ownership::Phase::Claimed);
    assert_eq!(after.ownership.child, None);
}

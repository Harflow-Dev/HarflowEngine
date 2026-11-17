use super::support::*;
use crate::{
    fixture_support::{A, B, later, world},
    task_writer_support::running,
};
use hf_domain::process::ProcessExit;
use hf_domain::task_lifecycle::{CancelIntent, ExecutionObservation, LifecycleDelta};
use hf_storage::{Error, LifecycleEventPublication};

#[test]
fn stale_and_wrong_fences_leave_lifecycle_revision_and_evidence_unchanged() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 2);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let claimed = world
        .store
        .apply_lifecycle(
            &initial.binding,
            None,
            0,
            &LifecycleDelta::OwnerClaimed(owner(30)),
            later(5),
        )
        .unwrap();
    let fence = claimed.ownership.fence().unwrap();

    let stale = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&fence),
            0,
            &LifecycleDelta::ChildRecorded(process(5100)),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(stale, Error::StaleOwnership));
    let mut wrong = fence.clone();
    wrong.attempt = id(99);
    let refused = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&wrong),
            1,
            &LifecycleDelta::ChildRecorded(process(5100)),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(
        refused,
        Error::StaleOwnership | Error::OwnershipConflict
    ));
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, 1);
    assert_eq!(after.ownership.child, None);
}

#[test]
fn cancellation_is_sticky_and_prevents_later_child_claim() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 3);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let claimed = world
        .store
        .apply_lifecycle(
            &initial.binding,
            None,
            0,
            &LifecycleDelta::OwnerClaimed(owner(40)),
            later(5),
        )
        .unwrap();
    let fence = claimed.ownership.fence().unwrap();
    let cancel = CancelIntent {
        sequence: 1,
        reason: "user_requested".into(),
        accepted_at: "2026-01-10T10:01:00Z".into(),
    };
    let cancelled = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&fence),
            1,
            &LifecycleDelta::CancelCommitted(cancel.clone()),
            later(5),
        )
        .unwrap();
    assert_eq!(cancelled.cancel, Some(cancel));
    let current_fence = cancelled.ownership.fence().unwrap();
    let conflicting_cancel = CancelIntent {
        sequence: 1,
        reason: "different_request".into(),
        accepted_at: "2026-01-10T10:01:00Z".into(),
    };
    let refusal = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&current_fence),
            2,
            &LifecycleDelta::CancelCommitted(conflicting_cancel),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(refusal, Error::OwnershipConflict));
    let child = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&current_fence),
            2,
            &LifecycleDelta::ChildRecorded(process(5200)),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(child, Error::OwnershipConflict));
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.cancel, cancelled.cancel);
    assert_eq!(after.revision, 2);
}

#[test]
fn observations_cannot_claim_complete_obligations_or_fabricate_terminal_event() {
    let world = world();
    let task = running(B);
    let request = request(&world.store, 4);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let child = claim_child(&world, &initial.binding);
    let fence = child.ownership.fence().unwrap();

    let observation = ExecutionObservation {
        exit: ProcessExit::Code(0),
        drained: true,
        input_closed: true,
        ..ExecutionObservation::default()
    };
    let complete = hf_domain::task_lifecycle::ExecutionObservation {
        obligations: hf_domain::task_lifecycle::Completeness::Complete,
        ..observation.clone()
    };
    let refused = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&fence),
            child.revision,
            &LifecycleDelta::ExecutionObserved(complete),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(refused, Error::LimitExceeded));

    let event = hf_storage::EventAppend {
        seq: 1,
        kind: "task_finished".into(),
        body: b"{}".to_vec(),
        raw_len: 2,
    };
    let capture = CaptureGate::new(false);
    let publication = LifecycleEventPublication {
        events: &[event],
        capture: &capture,
    };
    let refused = world
        .store
        .apply_lifecycle_events(
            &initial.binding,
            Some(&fence),
            child.revision,
            &LifecycleDelta::ObservationCommitted(observation),
            &publication,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(refused, Error::InvalidEventQuery));
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, child.revision);
    assert_eq!(
        after.ownership.phase,
        hf_domain::ownership::Phase::SpawnObserved
    );
    let events: i64 = world
        .db()
        .query_row(
            "SELECT count(*) FROM event_records WHERE task=?1",
            [B],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(events, 0);
}

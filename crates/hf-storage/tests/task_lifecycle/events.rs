use super::support::*;
use crate::{
    fixture_support,
    fixture_support::{A, B, later, world},
    task_writer_support::running,
};
use hf_application::task_lifecycle::CaptureProgress;
use hf_domain::task_lifecycle::{ExecutionObservation, LifecycleDelta};
use hf_storage::{Error, LifecycleEventPublication};

#[test]
fn events_cannot_commit_before_child_and_source_progress() {
    let world = world();
    let task = running(B);
    let request = request(&world.store, 10);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let body = b"{\"seq\":1,\"kind\":\"progress\"}".to_vec();
    let event = hf_storage::EventAppend {
        seq: 1,
        kind: "progress".into(),
        raw_len: body.len() as u64,
        body,
    };
    let capture = CaptureGate::new(false);
    let publication = LifecycleEventPublication {
        events: &[event],
        capture: &capture,
    };
    let error = world
        .store
        .apply_lifecycle_events(
            &initial.binding,
            None,
            0,
            &LifecycleDelta::OwnerClaimed(owner(50)),
            &publication,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidEventQuery));
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, 0);
    assert_eq!(after.ownership.owner, None);
    assert!(fixture_support::records(&world, B).is_empty());
    assert!(
        capture.requests.borrow().is_empty(),
        "pre-child rejection must precede capture admission"
    );
}

#[test]
fn event_append_rolls_back_when_lifecycle_evidence_cannot_commit() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 11);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let child = claim_child(&world, &initial.binding);
    let fence = child.ownership.fence().unwrap();
    world
        .raw()
        .execute_batch(
            "CREATE TRIGGER reject_lifecycle_update BEFORE UPDATE ON lifecycle_records
         BEGIN SELECT RAISE(ABORT, 'injected lifecycle update failure'); END;",
        )
        .unwrap();
    let body = b"{\"seq\":1,\"kind\":\"progress\"}".to_vec();
    let event = hf_storage::EventAppend {
        seq: 1,
        kind: "progress".into(),
        raw_len: body.len() as u64,
        body,
    };
    let observation = ExecutionObservation {
        stdout_end: 8,
        ..ExecutionObservation::default()
    };
    let capture = CaptureGate::new(false);
    let publication = LifecycleEventPublication {
        events: std::slice::from_ref(&event),
        capture: &capture,
    };
    let error = world
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
    assert!(matches!(error, Error::Sqlite(_)));
    assert_capture_request(
        &capture,
        &initial.binding,
        &process(5000),
        CaptureProgress {
            stdout_end: 8,
            stderr_end: 0,
            known_loss: false,
        },
    );
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, child.revision);
    assert_eq!(after.observation.normalized_event_bytes, 0);
    assert!(fixture_support::records(&world, A).is_empty());
}

#[test]
fn refused_capture_admission_rolls_back_events_and_offsets_with_exact_child_binding() {
    let world = world();
    let task = running(B);
    let request = request(&world.store, 12);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let child = claim_child(&world, &initial.binding);
    let fence = child.ownership.fence().unwrap();
    let body = b"{\"seq\":1,\"kind\":\"progress\"}".to_vec();
    let event = hf_storage::EventAppend {
        seq: 1,
        kind: "progress".into(),
        raw_len: body.len() as u64,
        body,
    };
    let capture = CaptureGate::new(true);
    let publication = LifecycleEventPublication {
        events: &[event],
        capture: &capture,
    };
    let observation = ExecutionObservation {
        stdout_end: 9,
        capture_known_loss: true,
        observation_gap: true,
        ..ExecutionObservation::default()
    };
    let error = world
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
    assert!(matches!(error, Error::InvalidOwnership));
    assert_capture_request(
        &capture,
        &initial.binding,
        &process(5000),
        CaptureProgress {
            stdout_end: 9,
            stderr_end: 0,
            known_loss: true,
        },
    );
    let after = world
        .store
        .read_lifecycle(&initial.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, child.revision);
    assert_eq!(after.observation, ExecutionObservation::default());
    assert!(fixture_support::records(&world, B).is_empty());
}

#[test]
fn committed_events_and_sticky_gap_offsets_advance_together() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 7);
    let initial = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let child = claim_child(&world, &initial.binding);
    let fence = child.ownership.fence().unwrap();
    let body = b"{\"seq\":1,\"kind\":\"progress\"}".to_vec();
    let event = hf_storage::EventAppend {
        seq: 1,
        kind: "progress".into(),
        raw_len: body.len() as u64,
        body,
    };
    let observation = ExecutionObservation {
        stdout_end: 12,
        observation_gap: true,
        omitted_events: 1,
        ..ExecutionObservation::default()
    };
    let without_capture = world
        .store
        .apply_lifecycle(
            &initial.binding,
            Some(&fence),
            child.revision,
            &LifecycleDelta::ObservationCommitted(observation.clone()),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(without_capture, Error::InvalidOwnership));
    let capture = CaptureGate::new(false);
    let publication = LifecycleEventPublication {
        events: std::slice::from_ref(&event),
        capture: &capture,
    };
    let committed = world
        .store
        .apply_lifecycle_events(
            &initial.binding,
            Some(&fence),
            child.revision,
            &LifecycleDelta::ObservationCommitted(observation.clone()),
            &publication,
            later(5),
        )
        .unwrap();
    assert_eq!(committed.observation.stdout_end, 12);
    assert_eq!(committed.observation.omitted_events, 1);
    assert_eq!(
        committed.observation.normalized_event_bytes,
        event.raw_len + 1
    );
    assert_eq!(fixture_support::records(&world, A).len(), 1);
    assert_capture_request(
        &capture,
        &initial.binding,
        &process(5000),
        CaptureProgress {
            stdout_end: 12,
            stderr_end: 0,
            known_loss: false,
        },
    );
    assert_monotonic_refusals(&world, &initial.binding, &committed);
}

fn assert_monotonic_refusals(
    world: &crate::fixture_support::World,
    binding: &hf_domain::task_lifecycle::LifecycleBinding,
    committed: &hf_domain::task_lifecycle::LifecycleEvidence,
) {
    let fence = committed.ownership.fence().unwrap();
    let mut regressed = committed.observation.clone();
    regressed.stdout_end = 11;
    let error = world
        .store
        .apply_lifecycle(
            binding,
            Some(&fence),
            committed.revision,
            &LifecycleDelta::ObservationCommitted(regressed),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::OwnershipConflict));
    let mut cleared = committed.observation.clone();
    cleared.observation_gap = false;
    let error = world
        .store
        .apply_lifecycle(
            binding,
            Some(&fence),
            committed.revision,
            &LifecycleDelta::ObservationCommitted(cleared),
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::LimitExceeded));
    let after = world
        .store
        .read_lifecycle(binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, committed.revision);
    assert_eq!(after.observation, committed.observation);
}

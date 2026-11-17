use super::support::*;
use crate::fixture_support::{A, B, one, world};
use hf_application::task_lifecycle::{
    CaptureAdmissionRefused, DurableCaptureAdmission, DurableCaptureRequest,
};
use hf_domain::{
    ownership::Phase,
    process::ProcessExit,
    task_lifecycle::{CancelIntent, ExecutionObservation, LifecycleDelta, ResultWitness},
};
use hf_storage::{Error, EventAppend};
use std::cell::{Cell, RefCell};

struct Capture {
    calls: Cell<usize>,
    child: RefCell<Option<hf_domain::process::ProcessIdentity>>,
}
impl DurableCaptureAdmission for Capture {
    fn admit(&self, request: &DurableCaptureRequest<'_>) -> Result<(), CaptureAdmissionRefused> {
        self.calls.set(self.calls.get() + 1);
        *self.child.borrow_mut() = Some(request.child.clone());
        if request.previous.stdout_end != 0
            || request.previous.stderr_end != 0
            || request.next.stdout_end != 0
            || request.next.stderr_end != 0
        {
            return Err(CaptureAdmissionRefused);
        }
        Ok(())
    }
}

fn finish_task(task: &mut hf_storage::TaskCreate, status: &str, exit: i64) {
    task.facts.status = status.into();
    task.facts.exit_code = Some(exit);
    task.facts.finished_at = Some("2026-01-10T10:05:00+00:00".into());
    task.facts.finished_at_us = Some(1_768_039_500_000_000);
    task.facts.output_drained = true;
    task.header = format!("{{\"status\":\"{status}\"}}").into_bytes();
    task.status = task.header.clone();
}

fn observation(exit: i32, success: bool) -> ExecutionObservation {
    ExecutionObservation {
        result: Some(ResultWitness {
            turn: id(90),
            digest: digest(b"result"),
            source_end: 0,
            success,
        }),
        exit: ProcessExit::Code(exit),
        drained: true,
        input_closed: true,
        ..ExecutionObservation::default()
    }
}

fn publish(
    world: &crate::fixture_support::World,
    spawned: &SpawnedTask,
    status: &str,
    exit: i64,
    cancelled: bool,
) -> Result<(), Error> {
    let mut task = task_for_spawn(A, &spawned.intent, &spawned.child);
    finish_task(&mut task, status, exit);
    let obs = observation(exit as i32, exit == 0);
    let body = format!("{{\"kind\":\"task_finished\",\"status\":\"{status}\"}}").into_bytes();
    let event = EventAppend {
        seq: 1,
        kind: "task_finished".into(),
        raw_len: body.len() as u64,
        body,
    };
    let capture = Capture {
        calls: Cell::new(0),
        child: RefCell::new(None),
    };
    let settlement = TerminalAdmissionGate {
        refuse: false,
        calls: Cell::new(0),
    };
    let projection = TerminalProjectionGate {
        refuse: false,
        calls: Cell::new(0),
    };
    let publication = terminal_publication(&task, &obs, &event, &capture, &settlement, &projection);
    let evidence = if cancelled {
        world.store.apply_lifecycle(
            &spawned.evidence.binding,
            Some(&spawned.fence),
            spawned.evidence.revision,
            &LifecycleDelta::CancelCommitted(CancelIntent {
                sequence: 1,
                reason: "user requested cancellation".into(),
                accepted_at: "2026-01-10T10:01:00+00:00".into(),
            }),
            later(5),
        )?
    } else {
        spawned.evidence.clone()
    };
    let fence = evidence.ownership.fence().unwrap();
    let result = world.store.publish_task_terminal(
        &evidence.binding,
        &fence,
        evidence.revision,
        &publication,
        later(5),
    );
    assert_eq!(capture.calls.get(), 1);
    assert_eq!(*capture.child.borrow(), Some(spawned.child.clone()));
    result.map(|_| ())
}

#[test]
fn terminal_success_publishes_typed_completion_and_eof_without_releasing_workflow() {
    let world = world();
    let spawned = spawn_task(&world, A, 81, "Finish this task.");
    let before = world.watch();
    publish(&world, &spawned, "completed", 0, false).unwrap();
    assert_eq!(world.watch(), before + 1);
    assert_eq!(
        one::<String>(
            &world.db(),
            "SELECT status FROM task_facts WHERE id=?1",
            [A]
        ),
        "completed"
    );
    assert_eq!(
        one::<i64>(
            &world.db(),
            "SELECT eof_seq FROM event_streams WHERE task=?1",
            [A]
        ),
        1
    );
    assert_eq!(
        one::<i64>(
            &world.db(),
            "SELECT drained FROM event_streams WHERE task=?1",
            [A]
        ),
        1
    );
    let evidence = world
        .store
        .read_lifecycle(&spawned.evidence.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(evidence.ownership.phase, Phase::SpawnObserved);
    assert_eq!(release_proof(&world, &spawned.evidence.binding), None);
}

#[test]
fn terminal_nonzero_exit_is_failed_and_unknown_exit_cannot_publish_any_projection() {
    let failed_world = world();
    let spawned = spawn_task(&failed_world, A, 82, "Fail in a known way.");
    publish(&failed_world, &spawned, "failed", 7, false).unwrap();
    assert_eq!(
        one::<i64>(
            &failed_world.db(),
            "SELECT exit_code FROM task_facts WHERE id=?1",
            [A]
        ),
        7
    );

    let world = world();
    let spawned = spawn_task(&world, B, 83, "Unknown process result.");
    let mut task = task_for_spawn(B, &spawned.intent, &spawned.child);
    finish_task(&mut task, "failed", 7);
    let mut obs = observation(7, false);
    obs.exit = ProcessExit::Unobserved;
    let event = EventAppend {
        seq: 1,
        kind: "task_finished".into(),
        raw_len: 13,
        body: b"task_finished".to_vec(),
    };
    let capture = Capture {
        calls: Cell::new(0),
        child: RefCell::new(None),
    };
    let settlement = TerminalAdmissionGate {
        refuse: false,
        calls: Cell::new(0),
    };
    let projection = TerminalProjectionGate {
        refuse: false,
        calls: Cell::new(0),
    };
    let publication = terminal_publication(&task, &obs, &event, &capture, &settlement, &projection);
    let before = world.watch();
    assert!(matches!(
        world.store.publish_task_terminal(
            &spawned.evidence.binding,
            &spawned.evidence.ownership.fence().unwrap(),
            spawned.evidence.revision,
            &publication,
            later(5)
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(world.watch(), before);
    assert_eq!(
        crate::fixture_support::count(&world.db(), "task_facts", "id", B),
        1
    );
    assert_eq!(
        crate::fixture_support::count(&world.db(), "event_streams", "task", B),
        0
    );
}

#[test]
fn committed_cancel_projects_cancelled_with_the_same_terminal_event_and_eof() {
    let world = world();
    let spawned = spawn_task(&world, A, 85, "Stop after accepting cancellation.");
    publish(&world, &spawned, "cancelled", 0, true).unwrap();
    assert_eq!(
        one::<String>(
            &world.db(),
            "SELECT status FROM task_facts WHERE id=?1",
            [A]
        ),
        "cancelled"
    );
    assert_eq!(
        one::<i64>(
            &world.db(),
            "SELECT eof_seq FROM event_streams WHERE task=?1",
            [A]
        ),
        1
    );
    assert_eq!(
        one::<i64>(
            &world.db(),
            "SELECT exit_code FROM task_facts WHERE id=?1",
            [A]
        ),
        0
    );
}

#[test]
fn terminal_refuses_unknown_drain_or_input_closure() {
    for missing in ["drain", "input"] {
        let world = world();
        let spawned = spawn_task(
            &world,
            A,
            if missing == "drain" { 86 } else { 87 },
            "Wait for complete capture.",
        );
        let mut task = task_for_spawn(A, &spawned.intent, &spawned.child);
        finish_task(&mut task, "completed", 0);
        let mut obs = observation(0, true);
        if missing == "drain" {
            obs.drained = false;
        } else {
            obs.input_closed = false;
        }
        let body = b"{\"kind\":\"task_finished\",\"status\":\"completed\"}".to_vec();
        let event = EventAppend {
            seq: 1,
            kind: "task_finished".into(),
            raw_len: body.len() as u64,
            body,
        };
        let capture = Capture {
            calls: Cell::new(0),
            child: RefCell::new(None),
        };
        let settlement = TerminalAdmissionGate {
            refuse: false,
            calls: Cell::new(0),
        };
        let projection = TerminalProjectionGate {
            refuse: false,
            calls: Cell::new(0),
        };
        let publication =
            terminal_publication(&task, &obs, &event, &capture, &settlement, &projection);
        let before = world.watch();
        assert!(matches!(
            world.store.publish_task_terminal(
                &spawned.evidence.binding,
                &spawned.fence,
                spawned.evidence.revision,
                &publication,
                later(5)
            ),
            Err(Error::InvalidOwnership)
        ));
        assert_eq!(settlement.calls.get(), 0);
        assert_eq!(world.watch(), before);
        assert_eq!(
            one::<String>(
                &world.db(),
                "SELECT status FROM task_facts WHERE id=?1",
                [A]
            ),
            "running"
        );
    }
}

#[test]
fn refused_terminal_settlement_rolls_back_task_event_and_observation() {
    let world = world();
    let spawned = spawn_task(&world, A, 84, "Wait for qualified settlement.");
    let mut task = task_for_spawn(A, &spawned.intent, &spawned.child);
    finish_task(&mut task, "completed", 0);
    let obs = observation(0, true);
    let body = b"{\"kind\":\"task_finished\",\"status\":\"completed\"}".to_vec();
    let event = EventAppend {
        seq: 1,
        kind: "task_finished".into(),
        raw_len: body.len() as u64,
        body,
    };
    let capture = Capture {
        calls: Cell::new(0),
        child: RefCell::new(None),
    };
    let settlement = TerminalAdmissionGate {
        refuse: true,
        calls: Cell::new(0),
    };
    let projection = TerminalProjectionGate {
        refuse: false,
        calls: Cell::new(0),
    };
    let publication = terminal_publication(&task, &obs, &event, &capture, &settlement, &projection);
    let before = world.watch();
    let previous_header: Vec<u8> = world
        .db()
        .query_row("SELECT header FROM task_headers WHERE id=?1", [A], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(matches!(
        world.store.publish_task_terminal(
            &spawned.evidence.binding,
            &spawned.fence,
            spawned.evidence.revision,
            &publication,
            later(5)
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(settlement.calls.get(), 1);
    assert_eq!(projection.calls.get(), 1);
    assert_eq!(world.watch(), before);
    assert_eq!(
        one::<String>(
            &world.db(),
            "SELECT status FROM task_facts WHERE id=?1",
            [A]
        ),
        "running"
    );
    let current_header: Vec<u8> = world
        .db()
        .query_row("SELECT header FROM task_headers WHERE id=?1", [A], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(current_header, previous_header);
    assert_eq!(
        crate::fixture_support::count(&world.db(), "event_streams", "task", A),
        0
    );
    let current = world
        .store
        .read_lifecycle(&spawned.evidence.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(current.revision, spawned.evidence.revision);
    assert_eq!(current.observation.exit, ProcessExit::Unobserved);
}

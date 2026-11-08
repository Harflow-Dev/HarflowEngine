#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The recovery-activity use case over scripted ports: the managed gate and its order, the
//! capture rules and the backend lookup that happens only after confirmed loss.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{ReadError, RecoveryOut, Resolution, recovery_activity},
};
use hf_domain::{
    ctl::{
        caller::{EnvMarkers, PidObservation, RefusalKind},
        recovery::{NOT_AVAILABLE, NOT_CONFIRMED, UNSUPPORTED, notice},
    },
    event_read::CaptureState,
};
use task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, World, caller, identity, record, root, task_id,
};

fn run(world: &World, markers: EnvMarkers, raw: &str) -> Result<RecoveryOut, ReadError> {
    recovery_activity(&world.reads(markers), &root(), raw)
}

fn one(status: &str, exit: Option<i64>, backend: &str, stored: Option<CaptureState>) -> World {
    let mut world = World::new();
    let mut found = record(1, status, exit, None);
    found.facts.backend = backend.to_string();
    world.tasks.push(found);
    if let Some(capture) = stored {
        world
            .feed
            .captures
            .insert(task_id(1).as_str().to_string(), capture);
    }
    world
}

fn capture(status: &str, exit: Option<i64>, stored: Option<CaptureState>) -> CaptureState {
    run(
        &one(status, exit, "claude", stored),
        NONE_MARKERS,
        task_id(1).as_str(),
    )
    .unwrap()
    .capture
}

#[test]
fn a_finished_task_with_an_exit_code_is_observed_whatever_the_stream_says() {
    for status in ["completed", "failed", "cancelled", "timed_out"] {
        for stored in [
            None,
            Some(CaptureState::Live),
            Some(CaptureState::LostWithOwner),
        ] {
            assert_eq!(
                capture(status, Some(0), stored),
                CaptureState::ObservedTerminal,
                "{status}"
            );
        }
    }
}

#[test]
fn an_unobserved_task_passes_a_stored_live_or_lost_capture_through_and_is_otherwise_unknown() {
    for (status, exit) in [
        ("running", None),
        ("running", Some(0)),
        ("completed", None),
        ("bogus", Some(0)),
    ] {
        assert_eq!(
            capture(status, exit, Some(CaptureState::Live)),
            CaptureState::Live
        );
        assert_eq!(
            capture(status, exit, Some(CaptureState::LostWithOwner)),
            CaptureState::LostWithOwner
        );
        assert_eq!(
            capture(status, exit, Some(CaptureState::ObservedTerminal)),
            CaptureState::Unknown
        );
        assert_eq!(
            capture(status, exit, Some(CaptureState::Unknown)),
            CaptureState::Unknown
        );
        assert_eq!(capture(status, exit, None), CaptureState::Unknown);
    }
}

#[test]
fn the_notice_depends_on_capture_loss_and_then_on_the_backend() {
    assert_eq!(notice(CaptureState::Live, "claude"), NOT_CONFIRMED);
    assert_eq!(notice(CaptureState::Unknown, "codex"), NOT_CONFIRMED);
    assert_eq!(
        notice(CaptureState::ObservedTerminal, "claude"),
        NOT_CONFIRMED
    );
    assert_eq!(notice(CaptureState::LostWithOwner, "codex"), UNSUPPORTED);
    assert_eq!(notice(CaptureState::LostWithOwner, "claude"), NOT_AVAILABLE);
    assert_eq!(
        NOT_CONFIRMED,
        "Transcript recovery requires confirmed capture loss"
    );
    assert_eq!(UNSUPPORTED, "Backend transcript recovery is unsupported");
}

#[test]
fn an_unregistered_backend_matters_only_after_confirmed_loss() {
    let lost = one("running", None, "nobody", Some(CaptureState::LostWithOwner));
    let error = run(&lost, NONE_MARKERS, task_id(1).as_str()).unwrap_err();
    assert_eq!(
        error,
        ReadError::InvalidParams(
            "unknown backend 'nobody'; expected one of ['antigravity', 'claude', 'codex', 'cursor', 'opencode', 'vibe']".into()
        )
    );
    for stored in [None, Some(CaptureState::Live)] {
        let world = one("running", None, "nobody", stored);
        assert!(run(&world, NONE_MARKERS, task_id(1).as_str()).is_ok());
    }
    for backend in [
        "antigravity",
        "claude",
        "codex",
        "cursor",
        "opencode",
        "vibe",
    ] {
        let world = one("running", None, backend, Some(CaptureState::LostWithOwner));
        assert!(
            run(&world, NONE_MARKERS, task_id(1).as_str()).is_ok(),
            "{backend}"
        );
    }
    let quoted = one("running", None, "it's", Some(CaptureState::LostWithOwner));
    let ReadError::InvalidParams(message) =
        run(&quoted, NONE_MARKERS, task_id(1).as_str()).unwrap_err()
    else {
        panic!()
    };
    assert!(
        message.starts_with("unknown backend \"it's\";"),
        "{message}"
    );
}

#[test]
fn a_prefix_is_echoed_and_an_exact_id_is_not() {
    let world = one("completed", Some(0), "claude", None);
    let exact = run(&world, NONE_MARKERS, task_id(1).as_str()).unwrap();
    assert_eq!(exact.resolved_from, None);
    let prefix = run(&world, NONE_MARKERS, "00000001").unwrap();
    assert_eq!(prefix.resolved_from.as_deref(), Some("00000001"));
    assert_eq!(prefix.task, task_id(1));
}

#[test]
fn a_pending_launch_and_a_vanished_record_are_unknown_tasks_with_a_plain_message() {
    let mut world = one("completed", Some(0), "claude", None);
    world.pending.push(task_id(5));
    let error = run(&world, NONE_MARKERS, task_id(5).as_str()).unwrap_err();
    assert_eq!(
        error,
        ReadError::Coded {
            code: "unknown_task",
            message: format!("unknown task_id: {}", task_id(5).as_str())
        }
    );
    world.vanish.push(task_id(1));
    let error = run(&world, NONE_MARKERS, task_id(1).as_str()).unwrap_err();
    assert_eq!(
        error,
        ReadError::Coded {
            code: "unknown_task",
            message: format!("unknown task_id: {}", task_id(1).as_str())
        }
    );
}

#[test]
fn unknown_and_ambiguous_ids_are_resolution_errors() {
    let mut world = one("completed", Some(0), "claude", None);
    world.tasks.push(record(2, "completed", Some(0), None));
    assert!(matches!(
        run(&world, NONE_MARKERS, "zz"),
        Err(ReadError::Resolution(_))
    ));
    assert!(matches!(
        run(&world, NONE_MARKERS, "ffffffff-ffff-4fff-8fff-ffffffffffff"),
        Err(ReadError::Resolution(Resolution::Unknown { .. }))
    ));
    assert!(matches!(
        run(&world, NONE_MARKERS, "bad/id"),
        Err(ReadError::InvalidTaskId(_))
    ));
}

#[test]
fn a_missing_root_is_a_resolution_error_that_creates_nothing_and_judges_the_syntax_first() {
    for class in [RootClass::Absent, RootClass::Empty] {
        let mut world = one("completed", Some(0), "claude", None);
        world.class = class;
        assert!(matches!(
            run(&world, NONE_MARKERS, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            Err(ReadError::Resolution(_))
        ));
        assert!(matches!(
            run(&world, NONE_MARKERS, "bad/id"),
            Err(ReadError::InvalidTaskId(_))
        ));
        assert_eq!(world.calls("snapshot"), 0);
    }
}

#[test]
fn the_syntax_is_judged_before_the_markers_and_a_non_empty_marker_refuses() {
    let world = one("completed", Some(0), "claude", None);
    assert!(matches!(
        run(&world, FULL_MARKER, "bad/id"),
        Err(ReadError::InvalidTaskId(_))
    ));
    let id = task_id(1);
    assert!(matches!(
        run(&world, FULL_MARKER, id.as_str()),
        Err(ReadError::Refused(_))
    ));
    assert!(run(&world, EMPTY_MARKER, id.as_str()).is_ok());
    assert!(run(&world, NONE_MARKERS, id.as_str()).is_ok());
}

#[test]
fn an_agent_is_refused_before_the_task_is_resolved_or_the_stream_read() {
    let mut world = one("completed", Some(0), "claude", None);
    world.callers = vec![caller(9, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    let error = run(&world, NONE_MARKERS, task_id(1).as_str()).unwrap_err();
    assert_eq!(error, ReadError::Refused(RefusalKind::Agent));
    assert_eq!(run(&world, NONE_MARKERS, "ffffffff").unwrap_err(), error);
    assert_eq!(world.calls("stream_capture"), 0);
    assert_eq!(world.calls("record"), 0);
}

#[test]
fn damage_refuses_and_a_foreign_root_is_judged_before_any_marker() {
    let mut world = one("completed", Some(0), "claude", None);
    world.damaged = true;
    assert_eq!(
        run(&world, NONE_MARKERS, task_id(1).as_str()).unwrap_err(),
        ReadError::Damaged
    );
    let mut world = one("completed", Some(0), "claude", None);
    let found = hf_domain::ctl::inspect::StateInspection::new(
        &root(),
        hf_domain::ctl::inspect::InspectStatus::Incompatible,
        hf_domain::ctl::inspect::InspectReason::UnmarkedNonempty,
        vec![],
    );
    world.class = RootClass::Refused(found);
    assert!(matches!(
        run(&world, FULL_MARKER, "bad/id"),
        Err(ReadError::State(_))
    ));
    assert!(matches!(
        run(&world, FULL_MARKER, task_id(1).as_str()),
        Err(ReadError::State(_))
    ));
}

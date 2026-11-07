#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The artifact page read over scripted ports: gate and its order, the check order after it,
//! ancestry, cursors, the page and byte bounds and the observation of targets.
mod task_artifacts_support;
mod task_reads_support;
use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{ArtifactCursorArg, ReadError, Resolution, UnknownReason},
};
use hf_domain::ctl::caller::{PidObservation, RefusalKind};
use task_artifacts_support::*;
use task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, caller, identity, record, task_id,
};
#[test]
fn the_order_is_gate_then_resolution_then_record_then_ancestry_then_manifest_then_items() {
    let world = world(3);
    read(
        &world,
        NONE_MARKERS,
        &request(20, ArtifactCursorArg::Absent),
    )
    .unwrap();
    assert_eq!(
        world.order(),
        [
            "classify", "view", "snapshot", "damaged", "related", "lookup", "record", "manifest",
            "items"
        ]
    );
}

#[test]
fn a_non_empty_marker_or_the_root_marker_refuses_and_an_empty_one_does_not() {
    let world = world(1);
    let outcome = read(&world, FULL_MARKER, &request(20, ArtifactCursorArg::Absent));
    assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
    assert!(
        read(
            &world,
            EMPTY_MARKER,
            &request(20, ArtifactCursorArg::Absent)
        )
        .is_ok()
    );
    let mut marked = request(20, ArtifactCursorArg::Absent);
    marked.root_task_marker = true;
    let outcome = read(&world, NONE_MARKERS, &marked);
    assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
}

#[test]
fn the_root_marker_does_not_make_an_empty_marker_count() {
    let world = world(1);
    let mut marked = request(20, ArtifactCursorArg::Absent);
    marked.root_task_marker = false;
    assert!(read(&world, EMPTY_MARKER, &marked).is_ok());
}

#[test]
fn the_id_syntax_is_checked_after_the_root_and_before_the_marker() {
    let world = world(1);
    let mut bad = request(20, ArtifactCursorArg::Absent);
    bad.task_id = "bad/id".into();
    bad.root_task_marker = true;
    let outcome = read(&world, NONE_MARKERS, &bad);
    assert!(
        matches!(outcome, Err(ReadError::InvalidTaskId(_))),
        "{outcome:?}"
    );
    assert_eq!(world.order(), ["classify"]);
}

#[test]
fn a_marker_refuses_before_the_process_view_and_nothing_else_is_read() {
    let world = world(1);
    let _ = read(&world, FULL_MARKER, &request(20, ArtifactCursorArg::Absent));
    assert_eq!(world.order(), ["classify"]);
}

#[test]
fn an_agent_is_refused_its_own_task_and_every_other() {
    let mut world = world(1);
    world.tasks.push(record(9, "running", None, Some(400)));
    world.callers = vec![caller(9, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    for ident in [task_id(9), task_id(1)] {
        let mut asked = request(20, ArtifactCursorArg::Absent);
        asked.task_id = ident.as_str().to_string();
        let outcome = read(&world, NONE_MARKERS, &asked);
        assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
    }
    assert_eq!(world.calls("lookup"), 0);
    assert_eq!(world.calls("items"), 0);
}

#[test]
fn the_gate_reads_the_bounded_caller_family() {
    let mut world = world(1);
    world.view = Ok(hf_domain::ctl::caller::ProcessView {
        pid: 500,
        session: 77,
        ancestors: (1_000..1_150).collect(),
    });
    read(
        &world,
        NONE_MARKERS,
        &request(20, ArtifactCursorArg::Absent),
    )
    .unwrap();
    assert_eq!(world.calls("related:77:100:100:1000-1099"), 1);
}

#[test]
fn a_damaged_root_is_refused_before_the_gate_runs() {
    let mut world = world(1);
    world.damaged = true;
    assert_eq!(
        read(
            &world,
            NONE_MARKERS,
            &request(20, ArtifactCursorArg::Absent)
        ),
        Err(ReadError::Damaged)
    );
    assert_eq!(world.calls("related"), 0);
}

#[test]
fn roots_without_a_store_say_unknown_task() {
    for class in [RootClass::Absent, RootClass::Empty] {
        let mut world = world(1);
        world.class = class;
        let outcome = read(
            &world,
            NONE_MARKERS,
            &request(20, ArtifactCursorArg::Absent),
        );
        assert_eq!(outcome, refused("Unknown task"));
        assert_eq!(world.order(), ["classify"]);
    }
}

#[test]
fn unknown_ids_and_short_prefixes_are_artifact_errors_and_other_prefix_failures_keep_their_codes() {
    let world = world(1);
    for ident in [task_id(2).as_str().to_string(), "zz".to_string()] {
        let mut asked = request(20, ArtifactCursorArg::Absent);
        asked.task_id = ident;
        assert_eq!(read(&world, NONE_MARKERS, &asked), refused("Unknown task"));
    }
    let mut asked = request(20, ArtifactCursorArg::Absent);
    asked.task_id = "nomatch0123".into();
    let outcome = read(&world, NONE_MARKERS, &asked);
    assert!(
        matches!(
            outcome,
            Err(ReadError::Resolution(Resolution::Unknown {
                reason: Some(UnknownReason::NoPrefixMatch),
                ..
            }))
        ),
        "{outcome:?}"
    );
}

#[test]
fn a_pending_launch_is_an_unknown_task() {
    let mut world = world(1);
    world.pending.push(task_id(5));
    let mut asked = request(20, ArtifactCursorArg::Absent);
    asked.task_id = task_id(5).as_str().to_string();
    assert_eq!(read(&world, NONE_MARKERS, &asked), refused("Unknown task"));
}

#[test]
fn the_limit_is_checked_after_resolution_and_before_the_record() {
    for limit in [0, 51, -1, i64::MAX] {
        let world = world(1);
        let outcome = read(
            &world,
            NONE_MARKERS,
            &request(limit, ArtifactCursorArg::Absent),
        );
        assert_eq!(
            outcome,
            refused("Artifact limit must be between 1 and 50"),
            "{limit}"
        );
        assert_eq!(world.calls("record"), 0);
        assert_eq!(world.calls("lookup"), 1);
    }
    let mut unknown = request(0, ArtifactCursorArg::Absent);
    unknown.task_id = task_id(2).as_str().to_string();
    assert_eq!(
        read(&world(1), NONE_MARKERS, &unknown),
        refused("Unknown task")
    );
    for limit in [1, 50] {
        assert!(
            read(
                &world(1),
                NONE_MARKERS,
                &request(limit, ArtifactCursorArg::Absent)
            )
            .is_ok()
        );
    }
}

#[test]
fn a_found_task_without_a_record_is_a_store_error_not_an_unknown_task() {
    let mut world = world(1);
    world.vanish.push(task_id(1));
    let outcome = read(
        &world,
        NONE_MARKERS,
        &request(20, ArtifactCursorArg::Absent),
    );
    assert!(matches!(outcome, Err(ReadError::Store(_))), "{outcome:?}");
}

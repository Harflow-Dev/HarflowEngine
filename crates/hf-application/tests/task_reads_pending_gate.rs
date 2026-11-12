#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The order of checks of the pending launch reads: markers, root availability and the
//! direct-only caller gate over every ordinary record. Every refusal of the family reports
//! unknown coverage.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        PendingCursorArg, PendingFailure, PendingLimits, ReadError, Reads, StoreFailure,
        ViewFailure, pending_page, pending_status,
    },
};
use hf_domain::ctl::inspect::{InspectReason, InspectStatus, StateInspection};
use std::time::Instant;
use task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS,
    pending_use::{
        agent, agent_world, check, first, page_error, page_with, request, request_of, small,
        status_error, status_of, undecidable, world,
    },
    root,
};

#[test]
fn a_marker_refuses_before_the_root_and_even_when_empty() {
    for markers in [FULL_MARKER, EMPTY_MARKER] {
        let mut world = agent_world();
        world.class = RootClass::Absent;
        let reads = world.reads(markers);
        let page = pending_page(
            &reads,
            &root(),
            &request_of(10),
            &PendingLimits::default(),
            &small,
        );
        assert_eq!(page, Err(ReadError::Pending(PendingFailure::Marker)));
        let status = pending_status(&reads, &root(), "ab", &PendingLimits::default());
        assert_eq!(status, Err(ReadError::Pending(PendingFailure::Marker)));
        assert!(world.order().is_empty(), "nothing is opened or classified");
    }
}

#[test]
fn a_marker_outranks_an_invalid_limit_and_cursor() {
    let world = world();
    let bad = request(0, PendingCursorArg::Malformed);
    let out = page_with(&world, FULL_MARKER, &bad, &PendingLimits::default(), &small);
    assert_eq!(out, Err(ReadError::Pending(PendingFailure::Marker)));
}

#[test]
fn a_root_that_is_not_a_store_is_unavailable_and_nothing_else_runs() {
    let inspection = StateInspection::new(
        &root(),
        InspectStatus::Incompatible,
        InspectReason::ForeignStore,
        Vec::new(),
    );
    for class in [
        RootClass::Absent,
        RootClass::Empty,
        RootClass::Refused(inspection),
    ] {
        let mut world = agent_world();
        world.class = class;
        assert_eq!(
            page_error(&world),
            ReadError::Pending(PendingFailure::Unavailable)
        );
        assert_eq!(world.order(), ["classify"], "no view, no snapshot");
        let mut world = agent_world();
        world.class = RootClass::Absent;
        assert_eq!(
            status_error(&world),
            ReadError::Pending(PendingFailure::Unavailable)
        );
    }
}

#[test]
fn the_usual_order_is_root_view_snapshot_check_gate_then_the_namespace() {
    let world = world();
    first(&world, 10).unwrap();
    assert_eq!(
        world.order(),
        [
            "classify",
            "view",
            "snapshot",
            "candidates_checked",
            "related",
            "pending_binding",
            "pending_namespace",
            "next_launch_name",
            "record_size",
            "pending_row",
            "next_launch_name"
        ]
    );
}

#[test]
fn the_status_order_ends_with_the_exact_row_and_resolves_no_prefix() {
    let world = world();
    let out = status_of(&world, "ab").unwrap();
    assert_eq!(out.id, "ab");
    assert_eq!(
        world.order(),
        [
            "classify",
            "view",
            "snapshot",
            "candidates_checked",
            "related",
            "record_size",
            "launch_name_exists",
            "pending_row"
        ]
    );
    assert_eq!(world.calls("lookup"), 0);
    let prefixed = status_of(&world, "a").unwrap();
    assert!(prefixed.outcome.is_none(), "a prefix is not resolved");
}

#[test]
fn the_check_of_every_record_precedes_the_caller_decision_and_discloses_nothing() {
    let mut world = agent_world();
    world.pending_world.checked = Ok(false);
    assert_eq!(page_error(&world), undecidable());
    assert_eq!(status_error(&world), undecidable());
    // An agent that could be seen is not reported: the caller is undecidable first.
    assert_eq!(world.calls("related"), 0);
    for call in [
        "pending_binding",
        "pending_namespace",
        "next_launch_name",
        "pending_row",
    ] {
        assert_eq!(world.calls(call), 0, "{call}");
    }
}

#[test]
fn an_exhausted_read_budget_in_the_check_is_undecidable() {
    let mut world = world();
    world.pending_world.charge = 101;
    let limits = PendingLimits {
        read_bytes: 100,
        ..PendingLimits::default()
    };
    let reads = world.reads(NONE_MARKERS);
    let request = request_of(10);
    assert_eq!(
        pending_page(&reads, &root(), &request, &limits, &small),
        Err(undecidable())
    );
    assert_eq!(
        pending_status(&reads, &root(), "ab", &limits),
        Err(undecidable())
    );
    // The check and the rows share one budget: what the check leaves is exactly one receipt.
    world.pending_world.charge = 90;
    let limits = PendingLimits {
        read_bytes: 90 + hf_domain::ctl::pending::LAUNCH_READ_CHARGE,
        ..PendingLimits::default()
    };
    let reads = world.reads(NONE_MARKERS);
    assert!(pending_page(&reads, &root(), &request, &limits, &small).is_ok());
    world.pending_world.charge = 91;
    let reads = world.reads(NONE_MARKERS);
    assert_eq!(
        pending_page(&reads, &root(), &request, &limits, &small),
        Err(check("pending_observation_bound", "read_budget"))
    );
}

#[test]
fn a_failed_store_check_leaves_the_caller_undecidable() {
    for failure in [
        StoreFailure::Busy,
        StoreFailure::Incompatible {
            reason: InspectReason::ForeignStore,
            message: "x".into(),
        },
    ] {
        let mut world = world();
        world.pending_world.checked = Err(failure);
        assert_eq!(page_error(&world), undecidable());
        assert_eq!(status_error(&world), undecidable());
    }
}

#[test]
fn a_live_agent_ancestor_is_an_agent_caller_before_any_launch_is_read() {
    let world = agent_world();
    assert_eq!(page_error(&world), agent());
    assert_eq!(status_error(&world), agent());
    assert_eq!(world.calls("pending_binding"), 0);
    assert_eq!(world.calls("next_launch_name"), 0);
}

#[test]
fn an_unknown_process_view_is_undecidable() {
    let mut world = world();
    world.view = Err(ViewFailure);
    assert_eq!(page_error(&world), undecidable());
    assert_eq!(status_error(&world), undecidable());
    assert_eq!(world.calls("next_launch_name"), 0);
}

#[test]
fn an_unreadable_store_is_unavailable_and_a_passed_deadline_is_the_read_budget() {
    let mut world = world();
    world.open_error = Some(StoreFailure::Busy);
    assert_eq!(
        page_error(&world),
        ReadError::Pending(PendingFailure::Unavailable)
    );
    let reads = Reads {
        roots: &world,
        store: &world,
        process: &world,
        markers: NONE_MARKERS,
        deadline: Instant::now(),
    };
    let out = pending_page(
        &reads,
        &root(),
        &request_of(10),
        &PendingLimits::default(),
        &small,
    );
    assert_eq!(out, Err(check("pending_observation_bound", "read_budget")));
    world.open_error = Some(StoreFailure::Incompatible {
        reason: InspectReason::ForeignStore,
        message: "x".into(),
    });
    assert_eq!(
        status_of(&world, "ab"),
        Err(ReadError::Pending(PendingFailure::Unavailable))
    );
}

/// A store whose exit check fails after the action ran, as the session's does on a passed
/// deadline (`failure`) or a replaced store.
struct FailsOnExit<'a> {
    world: &'a task_reads_support::World,
    failure: StoreFailure,
}

impl hf_application::ctl::task_reads::TaskStorePort for FailsOnExit<'_> {
    fn with_snapshot(
        &self,
        root: &hf_domain::ctl::state_root::StateRoot,
        deadline: Instant,
        action: &mut dyn FnMut(
            &dyn hf_application::ctl::task_reads::TaskSnapshotPort,
        ) -> Result<(), ReadError>,
    ) -> Result<(), ReadError> {
        let _ = self.world.with_snapshot(root, deadline, action);
        Err(ReadError::Store(self.failure.clone()))
    }
}

fn page_through(world: &task_reads_support::World, failure: StoreFailure) -> ReadError {
    let store = FailsOnExit { world, failure };
    let reads = Reads {
        roots: world,
        store: &store,
        process: world,
        markers: NONE_MARKERS,
        deadline: Instant::now(),
    };
    pending_page(
        &reads,
        &root(),
        &request_of(10),
        &PendingLimits::default(),
        &small,
    )
    .unwrap_err()
}

#[test]
fn a_passed_deadline_at_exit_keeps_an_undecidable_verdict_and_nothing_else() {
    let mut undecided = world();
    undecided.pending_world.checked = Ok(false);
    assert_eq!(page_through(&undecided, StoreFailure::Busy), undecidable());
    // Without that verdict a passed deadline is still the read budget.
    assert_eq!(
        page_through(&world(), StoreFailure::Busy),
        check("pending_observation_bound", "read_budget")
    );
    // A store replaced during the read outranks the verdict.
    let replaced = StoreFailure::Incompatible {
        reason: InspectReason::InfrastructureUnavailable,
        message: "replaced".into(),
    };
    assert_eq!(
        page_through(&undecided, replaced),
        ReadError::Pending(PendingFailure::Unavailable)
    );
}

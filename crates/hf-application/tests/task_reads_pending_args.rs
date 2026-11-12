#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The arguments of the pending launch reads after the gate: the limit, the cursor and its
//! binding, the task id, the namespace and the read budget of a row.
mod task_reads_support;

use hf_domain::ctl::pending::LAUNCH_READ_CHARGE;

use hf_application::ctl::task_reads::{
    PendingCursor, PendingCursorArg, PendingLimits, ReadError, StoreFailure, pending_status,
};
use task_reads_support::{
    NONE_MARKERS, World,
    pending_use::{
        agent_world, check, page_error, page_with, request, request_of, small, status_of,
        undecidable, world,
    },
    root,
};

#[test]
fn the_limit_is_judged_before_the_cursor_and_after_the_gate() {
    let limits = PendingLimits::default();
    for limit in [0, -1, 101, i64::MAX] {
        let world = world();
        let out = page_with(
            &world,
            NONE_MARKERS,
            &request(limit, PendingCursorArg::Malformed),
            &limits,
            &small,
        );
        assert_eq!(
            out,
            Err(check("invalid_pending_limit", "limit_1_to_100")),
            "{limit}"
        );
        assert_eq!(world.calls("candidates_checked"), 1, "the gate came first");
        assert_eq!(world.calls("pending_namespace"), 0);
    }
    for limit in [1, 50, 100] {
        assert!(page_with(&world(), NONE_MARKERS, &request_of(limit), &limits, &small).is_ok());
    }
    let mut undecided = agent_world();
    undecided.pending_world.checked = Ok(false);
    let out = page_with(
        &undecided,
        NONE_MARKERS,
        &request(0, PendingCursorArg::Absent),
        &limits,
        &small,
    );
    assert_eq!(out, Err(undecidable()), "the gate outranks the limit");
}

#[test]
fn a_malformed_cursor_is_a_shape_error_before_the_namespace() {
    let world = world();
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Malformed),
        &PendingLimits::default(),
        &small,
    );
    assert_eq!(out, Err(check("pending_cursor_invalid", "cursor_shape")));
    assert_eq!(world.calls("pending_namespace"), 0);
}

#[test]
fn another_root_or_limit_is_a_scope_error_before_the_namespace() {
    let world = world();
    let scope = check("pending_cursor_invalid", "cursor_scope");
    let mut other_root = cursor(&world, "ab");
    other_root.root = "x".repeat(64);
    assert_eq!(with_cursor(&world, other_root), Err(scope.clone()));
    let mut other_limit = cursor(&world, "ab");
    other_limit.limit = 11;
    let request = request(10, PendingCursorArg::Valid(other_limit));
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request,
        &PendingLimits::default(),
        &small,
    );
    assert_eq!(out.map(|_| ()), Err(scope));
    assert_eq!(world.calls("pending_namespace"), 0);
}

#[test]
fn another_store_or_namespace_revision_expires_the_cursor_before_the_position_is_looked_up() {
    let expired = check("pending_cursor_expired", "namespace_changed");
    let world = world();
    let mut other_store = cursor(&world, "ab");
    other_store.identity[3] = "9".into();
    assert_eq!(with_cursor(&world, other_store), Err(expired.clone()));
    let mut other_revision = cursor(&world, "ab");
    other_revision.revision = "m".repeat(64);
    assert_eq!(with_cursor(&world, other_revision), Err(expired));
    assert_eq!(world.calls("launch_name_exists"), 0);
    assert_eq!(world.calls("next_launch_name"), 0);
}

#[test]
fn a_position_that_names_no_launch_is_a_position_error() {
    let world = world();
    assert_eq!(
        with_cursor(&world, cursor(&world, "gone")),
        Err(check("pending_cursor_invalid", "cursor_position"))
    );
    assert_eq!(world.calls("next_launch_name"), 0);
    assert_eq!(with_cursor(&world, cursor(&world, "ab")), Ok(()));
}

#[test]
fn the_namespace_bound_and_a_store_failure_in_it_are_reported() {
    use hf_application::ctl::task_reads::NamespaceFailure;
    let mut world = world();
    world.pending_world.namespace = Err(NamespaceFailure::Bound);
    assert_eq!(
        page_error(&world),
        check("pending_observation_bound", "namespace_bound")
    );
    world.pending_world.namespace = Err(NamespaceFailure::Store(StoreFailure::Busy));
    assert_eq!(
        page_error(&world),
        check("pending_observation_unavailable", "namespace_unavailable")
    );
    assert_eq!(world.calls("next_launch_name"), 0);
}

#[test]
fn the_status_task_id_is_judged_after_the_gate_and_never_reads_a_row_when_invalid() {
    for raw in ["", "-a", "a b", "a/b", &"a".repeat(65)] {
        let world = world();
        assert_eq!(
            status_of(&world, raw),
            Err(check("invalid_task_id", "exact_task_id_required")),
            "{raw:?}"
        );
        assert_eq!(world.calls("candidates_checked"), 1);
        assert_eq!(world.calls("pending_row"), 0);
    }
    let mut undecided = World::new();
    undecided.pending_world.checked = Ok(false);
    assert_eq!(status_of(&undecided, ""), Err(undecidable()));
}

#[test]
fn a_receipt_read_charges_the_fixed_amount_to_the_budget() {
    let world = world();
    let limits = PendingLimits {
        read_bytes: LAUNCH_READ_CHARGE - 1,
        ..PendingLimits::default()
    };
    let reads = world.reads(NONE_MARKERS);
    assert_eq!(
        pending_status(&reads, &root(), "ab", &limits),
        Err(check("pending_observation_bound", "read_budget"))
    );
    let limits = PendingLimits {
        read_bytes: LAUNCH_READ_CHARGE,
        ..PendingLimits::default()
    };
    let reads = world.reads(NONE_MARKERS);
    assert!(pending_status(&reads, &root(), "ab", &limits).is_ok());
    let missing = pending_status(&reads, &root(), "zz", &limits).unwrap();
    assert!(
        missing.outcome.is_none(),
        "nothing is charged for a missing row"
    );
}
fn cursor(world: &World, last: &str) -> PendingCursor {
    let binding = world.pending_world.binding.clone().unwrap();
    PendingCursor {
        root: binding.root,
        identity: binding.identity.map(|part| part.to_string()),
        revision: "n".repeat(64),
        last: last.to_owned(),
        limit: 10,
        uncertain: false,
    }
}

fn with_cursor(world: &World, cursor: PendingCursor) -> Result<(), ReadError> {
    let request = request(cursor.limit, PendingCursorArg::Valid(cursor));
    page_with(
        world,
        NONE_MARKERS,
        &request,
        &PendingLimits::default(),
        &small,
    )
    .map(|_| ())
}

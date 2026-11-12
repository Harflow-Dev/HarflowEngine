#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The bounds of the page walk of `task-pending-page`: the read budget, the deadline and the
//! response size.
mod task_reads_support;

use hf_domain::ctl::pending::LAUNCH_READ_CHARGE;

use hf_application::ctl::task_reads::{
    PendingCursorArg, PendingFailure, PendingItem, PendingLimits, PendingPageOut, ReadError,
};
use task_reads_support::{
    NONE_MARKERS, World,
    pending_use::{ids, page_with, pending, recorded, request, small},
};

fn all_pending(count: usize) -> World {
    let mut world = World::new();
    world.pending_world.rows = (0..count)
        .map(|n| pending(&format!("t{n:03}"), "reserved"))
        .collect();
    world
}

fn ready(rows: Vec<hf_domain::ctl::pending::LaunchFacts>) -> World {
    let mut world = World::new();
    world.pending_world.rows = rows;
    world
}

fn bound(code: &'static str, check: &'static str) -> ReadError {
    ReadError::Pending(PendingFailure::Check { code, check })
}

#[test]
fn the_read_budget_stops_a_page_with_a_partial_answer() {
    let world = all_pending(5);
    let limits = PendingLimits {
        read_bytes: 2 * LAUNCH_READ_CHARGE + 5,
        ..PendingLimits::default()
    };
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &limits,
        &small,
    )
    .unwrap();
    assert_eq!(ids(&out), ["t000", "t001"]);
    assert_eq!(
        (out.examined_count, out.has_more, out.bound),
        (2, true, Some("read_budget"))
    );
    assert!(!out.coverage_complete);
    let cursor = out.next.unwrap();
    assert_eq!(cursor.last, "t001");
    assert!(cursor.uncertain, "a bound makes the cursor uncertain");
}

#[test]
fn a_read_budget_spent_before_any_row_is_examined_is_an_error() {
    let limits = PendingLimits {
        read_bytes: LAUNCH_READ_CHARGE - 1,
        ..PendingLimits::default()
    };
    let out = page_with(
        &all_pending(2),
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &limits,
        &small,
    );
    assert_eq!(out, Err(bound("pending_observation_bound", "read_budget")));
}

#[test]
fn a_passed_deadline_is_the_read_budget_bound() {
    let world = all_pending(2);
    let reads = hf_application::ctl::task_reads::Reads {
        roots: &world,
        store: &world,
        process: &world,
        markers: NONE_MARKERS,
        deadline: std::time::Instant::now(),
    };
    let out = hf_application::ctl::task_reads::pending_page(
        &reads,
        &task_reads_support::root(),
        &request(10, PendingCursorArg::Absent),
        &PendingLimits::default(),
        &small,
    );
    assert_eq!(out, Err(bound("pending_observation_bound", "read_budget")));
}

fn sized(size: usize) -> impl Fn(&PendingItem) -> usize {
    move |_| size
}

fn run_sized(world: &World, size: usize) -> Result<PendingPageOut, ReadError> {
    page_with(
        world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &PendingLimits::default(),
        &sized(size),
    )
}

#[test]
fn the_response_bound_cuts_a_page_after_the_items_that_fit() {
    let out = run_sized(&all_pending(3), 70_000).unwrap();
    assert_eq!(ids(&out), ["t000"]);
    assert_eq!(
        (out.examined_count, out.has_more, out.bound),
        (1, true, Some("response_bytes"))
    );
    assert!(!out.coverage_complete);
    assert!(out.next.as_ref().unwrap().uncertain);
}

#[test]
fn the_response_bound_with_nothing_examined_is_an_error() {
    let out = run_sized(&all_pending(3), 130_000);
    assert_eq!(
        out,
        Err(bound("pending_observation_bound", "response_bytes"))
    );
}

#[test]
fn the_response_bound_leaves_the_reserve_for_everything_else() {
    // Items stay below 128 KiB minus 8 KiB = 122,880 bytes, the empty list "[]" included.
    let fits = run_sized(&all_pending(1), 122_878).unwrap();
    assert_eq!(fits.items.len(), 1);
    let over = run_sized(&all_pending(1), 122_879);
    assert_eq!(
        over,
        Err(bound("pending_observation_bound", "response_bytes"))
    );
    // A second item pays two bytes of separator.
    let two = run_sized(&all_pending(2), 61_439).unwrap();
    assert_eq!((two.items.len(), two.bound), (1, Some("response_bytes")));
    let both = run_sized(&all_pending(2), 61_438).unwrap();
    assert_eq!((both.items.len(), both.bound), (2, None));
}

#[test]
fn filtered_rows_do_not_count_towards_the_response_and_a_bound_after_them_is_partial() {
    let world = ready(vec![recorded("a1"), pending("a2", "reserved")]);
    let out = run_sized(&world, 130_000).unwrap();
    assert!(out.items.is_empty());
    assert_eq!(
        (out.examined_count, out.bound, out.has_more),
        (1, Some("response_bytes"), true)
    );
    assert_eq!(out.next.as_ref().unwrap().last, "a1");
}

#[test]
fn a_spent_budget_stops_the_walk_before_any_later_receipt_is_read() {
    let world = all_pending(5);
    let limits = PendingLimits {
        read_bytes: LAUNCH_READ_CHARGE,
        ..PendingLimits::default()
    };
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &limits,
        &small,
    )
    .unwrap();
    assert_eq!(ids(&out), ["t000"]);
    // The second receipt was refused before it was fetched; the third was never reached.
    assert_eq!(world.calls("pending_row"), 1);
}

#[test]
fn records_are_not_charged_a_receipt_read() {
    let mut world = World::new();
    world.pending_world.rows = vec![recorded("t000"), recorded("t001")];
    let limits = PendingLimits {
        read_bytes: 0,
        ..PendingLimits::default()
    };
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &limits,
        &small,
    )
    .unwrap();
    assert_eq!((out.examined_count, out.bound), (2, None));
}

#[test]
fn a_record_wins_without_decoding_any_receipt_and_pays_its_stored_size() {
    let mut world = World::new();
    world.pending_world.rows = vec![recorded("t000"), recorded("t001")];
    world.pending_world.record_bytes = 7;
    let limits = PendingLimits {
        read_bytes: 14,
        ..PendingLimits::default()
    };
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &limits,
        &small,
    )
    .unwrap();
    assert_eq!((out.examined_count, out.bound), (2, None));
    assert_eq!(world.calls("pending_row"), 0);
    let tight = PendingLimits {
        read_bytes: 13,
        ..PendingLimits::default()
    };
    let out = page_with(
        &world,
        NONE_MARKERS,
        &request(10, PendingCursorArg::Absent),
        &tight,
        &small,
    )
    .unwrap();
    assert_eq!((out.examined_count, out.bound), (1, Some("read_budget")));
}

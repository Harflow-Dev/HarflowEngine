#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The page walk of `task-pending-page`: what counts as examined, how the cursor advances,
//! the bounds, and the sticky uncertainty that every later cursor inherits.
mod task_reads_support;

use hf_application::ctl::task_reads::{PendingItem, ReadError};
use hf_domain::ctl::pending::LaunchOutcome;
use task_reads_support::{
    World,
    pending_use::{after, first, ids, pending, recorded, refused, status_of},
};

fn ready(rows: Vec<hf_domain::ctl::pending::LaunchFacts>) -> World {
    let mut world = World::new();
    world.pending_world.rows = rows;
    world
}

fn names(count: usize) -> Vec<String> {
    (0..count).map(|n| format!("t{n:03}")).collect()
}

fn all_pending(count: usize) -> World {
    ready(
        names(count)
            .iter()
            .map(|id| pending(id, "reserved"))
            .collect(),
    )
}

#[test]
fn an_empty_namespace_is_a_complete_empty_page() {
    let out = first(&World::new(), 5).unwrap();
    assert!(out.items.is_empty());
    assert_eq!(
        (out.examined_count, out.has_more, out.coverage_complete),
        (0, false, true)
    );
    assert!(out.next.is_none() && out.bound.is_none());
    assert_eq!(out.namespace_revision, "n".repeat(64));
}

#[test]
fn a_page_stops_at_the_limit_and_the_cursor_resumes_after_the_last_row() {
    let world = all_pending(5);
    let one = first(&world, 2).unwrap();
    assert_eq!(ids(&one), ["t000", "t001"]);
    assert_eq!(
        (one.examined_count, one.has_more, one.coverage_complete),
        (2, true, false)
    );
    let cursor = one.next.clone().unwrap();
    assert_eq!(cursor.last, "t001");
    assert_eq!(cursor.limit, 2);
    assert_eq!(cursor.root, "r".repeat(64));
    assert_eq!(cursor.identity, ["1", "2", "3", "4"]);
    assert_eq!(cursor.revision, "n".repeat(64));
    assert!(!cursor.uncertain);
    let two = after(&world, &cursor).unwrap();
    assert_eq!(ids(&two), ["t002", "t003"]);
    let three = after(&world, two.next.as_ref().unwrap()).unwrap();
    assert_eq!(ids(&three), ["t004"]);
    assert!(three.next.is_none() && !three.has_more && three.coverage_complete);
    let asked = world.pending_world.asked.borrow().clone();
    assert_eq!(
        asked,
        [
            (None, 1),
            (Some("t000".to_owned()), 1),
            (Some("t001".to_owned()), 1),
            (Some("t001".to_owned()), 1),
            (Some("t002".to_owned()), 1),
            (Some("t003".to_owned()), 1),
            (Some("t003".to_owned()), 1),
            (Some("t004".to_owned()), 1)
        ]
    );
    // Rows are read one at a time: only the five examined receipts were ever decoded, and
    // the receipt after a full page was asked for by name only.
    assert_eq!(world.calls("pending_row"), 5);
}

#[test]
fn a_page_that_ends_exactly_at_the_last_row_has_no_more() {
    let out = first(&all_pending(3), 3).unwrap();
    assert_eq!(
        (out.items.len(), out.has_more, out.coverage_complete),
        (3, false, true)
    );
    assert!(out.next.is_none());
}

#[test]
fn filtered_candidates_count_as_examined_and_the_cursor_moves_past_them() {
    let world = ready(vec![
        recorded("a1"),
        pending("a2", "spawned"),
        pending("a3", "granted"),
        pending("a4", "granted"),
    ]);
    let out = first(&world, 1).unwrap();
    assert_eq!(ids(&out), ["a3"]);
    assert_eq!(out.examined_count, 3);
    assert!(out.has_more);
    assert_eq!(out.next.as_ref().unwrap().last, "a3");
    let rest = after(&world, out.next.as_ref().unwrap()).unwrap();
    assert_eq!(
        (ids(&rest), rest.examined_count, rest.has_more),
        (vec!["a4".to_owned()], 1, false)
    );
}

#[test]
fn a_page_of_only_filtered_candidates_is_empty_but_complete() {
    let world = ready(vec![recorded("a1"), pending("a2", "settled")]);
    let out = first(&world, 1).unwrap();
    assert!(out.items.is_empty());
    assert_eq!(
        (out.examined_count, out.has_more, out.coverage_complete),
        (2, false, true)
    );
    assert!(out.next.is_none());
}

#[test]
fn candidates_after_the_limit_are_not_examined_until_the_next_page() {
    let world = ready(vec![pending("a1", "reserved"), recorded("a2")]);
    let out = first(&world, 1).unwrap();
    assert_eq!((out.examined_count, out.has_more), (1, true));
    let rest = after(&world, out.next.as_ref().unwrap()).unwrap();
    assert!(rest.items.is_empty());
    assert_eq!(
        (rest.examined_count, rest.has_more, rest.coverage_complete),
        (1, false, true)
    );
}

#[test]
fn a_page_examines_at_most_a_hundred_candidates_and_that_is_not_uncertainty() {
    let world = all_pending(101);
    let out = first(&world, 100).unwrap();
    assert_eq!(
        (out.items.len(), out.examined_count, out.has_more),
        (100, 100, true)
    );
    assert!(!out.next.as_ref().unwrap().uncertain);
    assert!(out.bound.is_none());
    let tail = after(&world, out.next.as_ref().unwrap()).unwrap();
    assert_eq!(ids(&tail), ["t100"]);
    assert!(tail.coverage_complete);
    // Filtered candidates fill the examined bound too.
    let rows = names(150).iter().map(|id| recorded(id)).collect();
    let world = ready(rows);
    let out = first(&world, 5).unwrap();
    assert_eq!(
        (out.items.len(), out.examined_count, out.has_more),
        (0, 100, true)
    );
    assert_eq!(out.next.as_ref().unwrap().last, "t099");
    let tail = after(&world, out.next.as_ref().unwrap()).unwrap();
    assert_eq!((tail.examined_count, tail.has_more), (50, false));
}

#[test]
fn prefix_nested_ids_both_list_because_resumption_uses_the_full_filename() {
    let world = ready(vec![pending("ab", "reserved"), pending("ab-c", "reserved")]);
    let one = first(&world, 1).unwrap();
    assert_eq!(ids(&one), ["ab-c"]);
    let two = after(&world, one.next.as_ref().unwrap()).unwrap();
    assert_eq!(ids(&two), ["ab"]);
}

#[test]
fn a_refused_row_is_listed_and_makes_every_later_cursor_uncertain() {
    let world = ready(vec![
        pending("a1", "reserved"),
        refused("a2"),
        pending("a3", "reserved"),
        pending("a4", "reserved"),
    ]);
    let one = first(&world, 1).unwrap();
    assert!(!one.next.as_ref().unwrap().uncertain);
    let two = after(&world, one.next.as_ref().unwrap()).unwrap();
    assert!(matches!(
        two.items[0],
        PendingItem::Refused {
            code: "format",
            check: "format",
            ..
        }
    ));
    assert!(two.next.as_ref().unwrap().uncertain);
    let three = after(&world, two.next.as_ref().unwrap()).unwrap();
    assert!(
        three.next.as_ref().unwrap().uncertain,
        "sticky across cursors"
    );
    let last = after(&world, three.next.as_ref().unwrap()).unwrap();
    assert!(!last.has_more);
    assert!(
        !last.coverage_complete,
        "the last page inherits the uncertainty"
    );
}

#[test]
fn invalid_names_make_the_page_and_its_cursor_uncertain() {
    let mut world = all_pending(3);
    world.pending_world.namespace = Ok(hf_application::ctl::task_reads::Namespace {
        revision: "n".repeat(64),
        invalid: 2,
    });
    let out = first(&world, 2).unwrap();
    assert_eq!(out.invalid_name_count, 2);
    assert!(out.next.as_ref().unwrap().uncertain);
    let last = after(&world, out.next.as_ref().unwrap()).unwrap();
    assert!(!last.coverage_complete && last.next.is_none());
}

#[test]
fn an_uncertain_cursor_keeps_a_page_without_refusals_unknown() {
    let world = all_pending(2);
    let mut cursor = first(&world, 1).unwrap().next.unwrap();
    cursor.uncertain = true;
    let out = after(&world, &cursor).unwrap();
    assert!(!out.coverage_complete && !out.has_more);
}

#[test]
fn a_store_failure_while_reading_rows_is_unavailable() {
    let mut world = all_pending(2);
    world.pending_world.rows_failure = Some(hf_application::ctl::task_reads::StoreFailure::Busy);
    assert_eq!(
        first(&world, 5),
        Err(ReadError::Pending(
            hf_application::ctl::task_reads::PendingFailure::Check {
                code: "pending_observation_unavailable",
                check: "source_unavailable"
            }
        ))
    );
}

#[test]
fn the_status_of_an_id_reports_each_disposition() {
    let world = ready(vec![
        pending("p1", "granted"),
        recorded("r1"),
        refused("f1"),
        pending("s1", "spawned"),
    ]);
    let outcome = |id: &str| status_of(&world, id).unwrap().outcome;
    assert!(matches!(outcome("p1"), Some(LaunchOutcome::Pending(_))));
    assert_eq!(outcome("r1"), Some(LaunchOutcome::Recorded));
    assert_eq!(
        outcome("f1"),
        Some(LaunchOutcome::Refused {
            code: "format",
            check: "format"
        })
    );
    assert_eq!(outcome("s1"), Some(LaunchOutcome::Inconsistent));
    assert_eq!(outcome("zz"), None);
    assert_eq!(status_of(&world, "p1").unwrap().id, "p1");
}

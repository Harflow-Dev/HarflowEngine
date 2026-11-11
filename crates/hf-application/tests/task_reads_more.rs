#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Pending launches, batch rules, count caps and tests written to the behaviour of review round 1.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{CursorArg, PageMode, PageRequest, ReadError, list, status, status_batch},
};
use hf_domain::ctl::caller::{PidObservation, RefusalKind};
use hf_domain::ctl::inspect::{InspectReason, InspectStatus, StateInspection};
use task_reads_support::{NONE_MARKERS, World, caller, identity, page, record, root, task_id};

fn request(mode: PageMode) -> PageRequest {
    PageRequest {
        mode,
        limit: 10,
        cursor: CursorArg::Absent,
        active_only: false,
        session: None,
    }
}

fn world() -> World {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), None));
    world
}

/// the range check moves after root classification.
#[test]
fn plain_page_limit_is_checked_after_root_classification() {
    let inspection = StateInspection::new(
        &root(),
        InspectStatus::Incompatible,
        InspectReason::ForeignStore,
        Vec::new(),
    );
    let mut world = world();
    world.class = RootClass::Refused(inspection.clone());
    let mut asked = request(PageMode::Plain);
    asked.limit = 0;
    assert_eq!(
        page(&world.reads(NONE_MARKERS), &root(), &asked).map(|_| ()),
        Err(ReadError::State(inspection))
    );
}

/// ids resolve in request order.
#[test]
fn explicit_ids_resolve_in_request_order() {
    let world = world();
    let ids = vec!["nomatch0123".to_string(), "bad/id".to_string()];
    let out = page(
        &world.reads(NONE_MARKERS),
        &root(),
        &request(PageMode::TaskIds(ids)),
    );
    assert!(matches!(out, Err(ReadError::Resolution(_))), "{out:?}");
}

fn pending(raw: &str) -> ReadError {
    ReadError::PendingLaunch {
        raw: raw.to_string(),
        state: "waiting_for_resources".into(),
        not_started_reason: None,
    }
}

#[test]
fn a_pending_launch_is_an_error_for_status_and_dropped_from_explicit_ids() {
    let mut world = world();
    world.pending = vec![task_id(5)];
    let reads = world.reads(NONE_MARKERS);
    let raw = task_id(5).as_str().to_string();
    assert_eq!(
        status(&reads, &root(), &raw).map(|_| ()),
        Err(pending(&raw))
    );
    let ids = vec![raw.clone(), task_id(1).as_str().to_string()];
    let out = page(&reads, &root(), &request(PageMode::TaskIds(ids))).unwrap();
    assert_eq!(out.items.len(), 1);
    let batch = status_batch(&reads, &root(), std::slice::from_ref(&raw)).unwrap();
    assert_eq!(batch.errors, vec![(raw.clone(), pending(&raw))]);
}

#[test]
fn batch_dedupes_bounds_and_orders_errors() {
    let world = world();
    let reads = world.reads(NONE_MARKERS);
    let one = task_id(1).as_str().to_string();
    let out = status_batch(
        &reads,
        &root(),
        &[
            one.clone(),
            one.clone(),
            "bad/id".into(),
            task_id(9).as_str().into(),
        ],
    )
    .unwrap();
    assert_eq!(out.rows.len(), 1);
    assert_eq!(out.errors.len(), 2);
    assert_eq!(out.errors[0].0, "bad/id");
    let none: Vec<String> = Vec::new();
    assert!(matches!(
        status_batch(&reads, &root(), &none),
        Err(ReadError::InvalidParams(_))
    ));
    let many: Vec<String> = (0..101).map(|n| format!("x{n}")).collect();
    assert!(matches!(
        status_batch(&reads, &root(), &many),
        Err(ReadError::InvalidParams(_))
    ));
    assert_eq!(
        world.calls("classify"),
        3,
        "the root is classified before the bounds are checked"
    );
    let repeated = vec![one; 150];
    assert!(status_batch(&reads, &root(), &repeated).is_ok());
}

#[test]
fn over_4096_unobserved_rows_no_count_is_claimed() {
    let mut world = World::new();
    world.unobserved_total = Some(4097);
    let out = page(
        &world.reads(NONE_MARKERS),
        &root(),
        &request(PageMode::Plain),
    )
    .unwrap();
    let counts = out.counts.unwrap();
    assert_eq!(
        (
            counts.complete,
            counts.active,
            counts.active_root,
            counts.attention_root
        ),
        (false, None, None, None)
    );
    assert_eq!(world.calls("unobserved_rows"), 0);
    assert!(out.withheld.is_none());
    world.unobserved_total = Some(4096);
    let counts = page(
        &world.reads(NONE_MARKERS),
        &root(),
        &request(PageMode::Plain),
    )
    .unwrap()
    .counts
    .unwrap();
    assert!(counts.complete && counts.active_root == Some(0));
}

#[test]
fn one_command_takes_one_snapshot_one_view_and_one_detection() {
    let mut world = World::new();
    for n in 1..=230 {
        world.tasks.push(record(n, "completed", Some(0), None));
    }
    let reads = world.reads(NONE_MARKERS);
    assert_eq!(list(&reads, &root(), None).unwrap().len(), 230);
    assert!(world.calls("batch") >= 3);
    assert_eq!(
        (
            world.calls("snapshot"),
            world.calls("view"),
            world.calls("related")
        ),
        (1, 1, 1)
    );
    let ids: Vec<String> = (1..=50).map(|n| task_id(n).as_str().to_string()).collect();
    let batch = status_batch(&reads, &root(), &ids).unwrap();
    assert_eq!(batch.rows.len(), 50);
    assert_eq!(
        (
            world.calls("snapshot"),
            world.calls("view"),
            world.calls("related")
        ),
        (2, 2, 2)
    );
}

#[test]
fn plain_page_limit_outside_one_to_a_hundred_is_a_range_error() {
    let world = world();
    for limit in [0, 101, -4] {
        let mut asked = request(PageMode::Plain);
        asked.limit = limit;
        assert!(matches!(
            page(&world.reads(NONE_MARKERS), &root(), &asked),
            Err(ReadError::InvalidParams(_))
        ));
    }
    let mut ids = request(PageMode::TaskIds(vec![task_id(1).as_str().to_string()]));
    ids.limit = 0;
    ids.cursor = CursorArg::Malformed;
    assert!(page(&world.reads(NONE_MARKERS), &root(), &ids).is_ok());
}

#[test]
fn a_malformed_cursor_on_a_root_without_a_store_is_invalid_params() {
    let mut world = world();
    world.class = RootClass::Absent;
    let reads = world.reads(NONE_MARKERS);
    let mut asked = request(PageMode::Plain);
    asked.cursor = CursorArg::Malformed;
    assert!(matches!(
        page(&reads, &root(), &asked),
        Err(ReadError::InvalidParams(_))
    ));
    let mut ids = request(PageMode::TaskIds(vec![task_id(1).as_str().to_string()]));
    ids.cursor = CursorArg::Malformed;
    assert!(page(&reads, &root(), &ids).is_ok());
}

#[test]
fn successor_limit_comes_after_authority_and_the_predecessor() {
    let mut asked = request(PageMode::Successors(task_id(1).as_str().to_string()));
    asked.limit = 0;
    let mut gated = world();
    gated.callers = vec![caller(9, 400)];
    gated
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    assert_eq!(
        page(&gated.reads(NONE_MARKERS), &root(), &asked).map(|_| ()),
        Err(ReadError::Refused(RefusalKind::Agent))
    );
    let direct = world();
    assert!(matches!(
        page(&direct.reads(NONE_MARKERS), &root(), &asked),
        Err(ReadError::InvalidParams(_))
    ));
    assert_eq!(
        direct.calls("lookup"),
        1,
        "the predecessor is resolved first"
    );
    let mut missing = request(PageMode::Successors(task_id(7).as_str().to_string()));
    missing.limit = 0;
    assert!(matches!(
        page(&direct.reads(NONE_MARKERS), &root(), &missing),
        Err(ReadError::Resolution(_))
    ));
}

#[test]
fn a_failed_process_view_still_leaves_the_snapshot_and_the_damage_check() {
    use hf_application::ctl::task_reads::{StoreFailure, ViewFailure};
    use hf_domain::ctl::caller::RefusalKind;
    let mut damaged = world();
    damaged.view = Err(ViewFailure);
    damaged.damaged = true;
    let reads = damaged.reads(NONE_MARKERS);
    let out = page(&reads, &root(), &request(PageMode::Plain)).unwrap();
    assert!(
        out.withheld.is_some(),
        "the damage check ran and outranks the gate"
    );
    assert_eq!(damaged.calls("snapshot"), 1);
    assert_eq!(damaged.calls("damaged"), 1);
    let mut healthy = world();
    healthy.view = Err(ViewFailure);
    let reads = healthy.reads(NONE_MARKERS);
    let refused = Err(ReadError::Refused(RefusalKind::Undecidable));
    assert_eq!(
        page(&reads, &root(), &request(PageMode::Plain)).map(|_| ()),
        refused
    );
    assert_eq!(
        status(&reads, &root(), task_id(1).as_str()).map(|_| ()),
        refused
    );
    assert_eq!(list(&reads, &root(), None).map(|_| ()), refused);
    assert_ne!(
        list(&reads, &root(), None).map(|_| ()),
        Err(ReadError::Store(StoreFailure::Busy))
    );
    assert_eq!(
        healthy.calls("snapshot"),
        4,
        "each read still opened its snapshot"
    );
}

fn drift() -> ReadError {
    ReadError::Store(
        hf_application::ctl::task_reads::StoreFailure::Incompatible {
            reason: hf_domain::ctl::inspect::InspectReason::InfrastructureUnavailable,
            message: "Task history cannot be read".to_string(),
        },
    )
}

#[test]
fn a_found_id_with_no_record_is_state_incompatible_never_unknown_or_dropped() {
    let mut world = world();
    world.vanish = vec![task_id(1)];
    let reads = world.reads(NONE_MARKERS);
    let raw = task_id(1).as_str().to_string();
    assert_eq!(status(&reads, &root(), &raw).map(|_| ()), Err(drift()));
    assert_eq!(
        status_batch(&reads, &root(), std::slice::from_ref(&raw)).map(|_| ()),
        Err(drift()),
        "a drifted store fails the batch, not one row"
    );
    let ids = request(PageMode::TaskIds(vec![raw]));
    assert_eq!(page(&reads, &root(), &ids).map(|_| ()), Err(drift()));
}

/// Related headers have their own budget, `PAGE_BYTES - ROW_BYTES` (128 KiB), never more.
#[test]
fn related_headers_stop_at_their_own_budget() {
    let mut world = World::new();
    for n in 1..=40 {
        let mut made = record(n, "completed", Some(0), None);
        made.header = format!(r#"{{"task_id":"x","pad":"{}"}}"#, "p".repeat(5000)).into_bytes();
        if n > 1 {
            made.facts.continuation_of = Some(task_id(n - 1));
        }
        world.tasks.push(made);
    }
    let reads = world.reads(NONE_MARKERS);
    let out = page(
        &reads,
        &root(),
        &PageRequest {
            limit: 1,
            ..request(PageMode::Plain)
        },
    )
    .unwrap();
    assert_eq!(out.items.len(), 1);
    let total: usize = out.related.iter().map(task_reads_support::real_size).sum();
    assert!(total <= 128 * 1024, "{total}");
    assert!(
        out.related.len() < 32 && out.related.len() >= 20,
        "{}",
        out.related.len()
    );
    // The next header would have crossed the line, so the walk stopped there.
    assert!(total + task_reads_support::real_size(&out.related[0]) > 128 * 1024);
}

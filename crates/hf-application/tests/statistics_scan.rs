#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The statistics-page use case over scripted ports.
mod statistics_support;
mod task_reads_support;

#[allow(unused_imports)]
use hf_application::ctl::{state_inspect::RootClass, task_reads::*};
#[allow(unused_imports)]
use hf_domain::ctl::{caller::*, statistics::*, statistics_time::*};
#[allow(unused_imports)]
use statistics_support::*;
#[allow(unused_imports)]
use std::time::Duration;
#[allow(unused_imports)]
use task_reads_support::*;

#[test]
fn the_scan_phase_expires_a_cursor_walk_and_leaves_a_fresh_read_unavailable() {
    let coverage = coded(
        "snapshot_expired",
        "Statistics source coverage changed; restart the page walk",
    );
    let cases = [
        (
            ScanFailure::Deadline,
            "discovery_budget",
            "No source classification fits the foreground deadline",
        ),
        (
            ScanFailure::Limit,
            "source_oversized",
            "Statistics source exceeds the bounded read budget",
        ),
    ];
    for (failure, code, message) in cases {
        let mut world = world(vec![done("a", 0)]);
        world.feed.scan_failure = Some((0, failure));
        assert_eq!(
            unavailable(run(&world, &request(1, StatsCursorArg::Absent), &SMALL)),
            diag(code, message)
        );
        world.feed.batches.set(0);
        assert_eq!(
            run(&world, &request(1, bound(Some("g"), 0)), &SMALL).unwrap_err(),
            coverage
        );
    }
}

#[test]
fn a_busy_or_failing_store_is_never_turned_into_a_page_or_an_expiry() {
    let incompatible = StoreFailure::Incompatible {
        reason: hf_domain::ctl::inspect::InspectReason::UnmarkedNonempty,
        message: "m".into(),
    };
    for failure in [StoreFailure::Busy, incompatible] {
        let mut world = world(vec![done("a", 0)]);
        world.feed.scan_failure = Some((0, ScanFailure::Store(failure.clone())));
        for cursor in [StatsCursorArg::Absent, bound(Some("g"), 0)] {
            world.feed.batches.set(0);
            assert_eq!(
                run(&world, &request(1, cursor), &SMALL).unwrap_err(),
                ReadError::Store(failure.clone())
            );
        }
    }
}

#[test]
fn a_failure_in_a_later_batch_ends_the_scan_the_same_way() {
    let mut world = world((0..5).map(|n| done(&format!("t{n}"), n)).collect());
    world.feed.batch_rows = 2;
    world.feed.scan_failure = Some((2, ScanFailure::Deadline));
    // Four rows were read before the failing batch: the page is preparing, not unavailable.
    let out = run(&world, &request(1, StatsCursorArg::Absent), &SMALL).unwrap();
    assert_eq!(out, StatsOut::Preparing(Vec::new()));
    assert_eq!(world.feed.batches.get(), 3);
}

#[test]
fn ten_thousand_sources_are_scanned_and_one_more_is_a_history_limit() {
    for (count, over) in [(10_000, false), (10_001, true)] {
        let world = world((0..count).map(|n| done(&format!("t{n:05}"), 0)).collect());
        let out = run(&world, &request(1, StatsCursorArg::Absent), &SMALL);
        if over {
            assert_eq!(
                unavailable(out),
                diag("history_limit", "Statistics source history limit exceeded")
            );
            let walk = run(&world, &request(1, bound(Some("g"), 0)), &SMALL).unwrap_err();
            assert_eq!(
                walk,
                coded(
                    "snapshot_expired",
                    "Statistics source coverage changed; restart the page walk"
                )
            );
        } else {
            assert_eq!(ready(out).meta.eligible, 10_000);
        }
        // 50 full batches, then the batch that ends the scan or passes the limit.
        // 10,001 valid rows are judged before the scan reads a batch.
        assert_eq!(world.feed.batches.get(), if over { 0 } else { 51 });
    }
}

#[test]
fn error_rows_are_projection_failures_even_with_a_cursor() {
    let mut world = world(vec![done("a", 0), done("c", 1)]);
    world.feed.rows.insert(1, ScanRow::Oversized("b".into()));
    let expected = diag("source_oversized", "task:b");
    assert_eq!(
        unavailable(run(&world, &request(1, StatsCursorArg::Absent), &SMALL)),
        expected
    );
    assert_eq!(
        unavailable(run(&world, &request(1, bound(Some("g"), 0)), &SMALL)),
        expected
    );
    let mut world = self::world(vec![done("a", 0), stat_facts("b", "bogus", FIRST, None)]);
    world.feed.batch_rows = 1;
    assert_eq!(
        unavailable(run(&world, &request(1, bound(Some("g"), 0)), &SMALL)),
        diag("source_unavailable", "task:b")
    );
    let big = Sizes {
        oversized: true,
        ..SMALL
    };
    let world = self::world(vec![done("a", 0)]);
    assert_eq!(
        unavailable(run(&world, &request(1, StatsCursorArg::Absent), &big)),
        diag("projection_oversized", "task:a")
    );
}

#[test]
fn an_incomplete_projection_is_unavailable_before_the_generation_is_checked() {
    let mut orphan = done("c1", 0);
    orphan.continuation_of = Some(hf_domain::task_read::TaskId::new("ghost".into()).unwrap());
    let world = world(vec![orphan]);
    let out = unavailable(run(&world, &request(1, bound(Some("stale"), 99)), &SMALL));
    assert_eq!(
        out,
        diag(
            "ownership_unavailable",
            "Task ancestor metadata is unavailable"
        )
    );
    assert_eq!(world.calls("statistics_generation"), 0);
}

#[test]
fn a_window_without_relevant_tasks_uses_the_empty_generation_and_asks_nothing_of_the_store() {
    let mut running = stat_facts("r", "running", FIRST, None);
    running.exit_code = None;
    let world = world(vec![running]);
    let page = ready(run(&world, &request(1, StatsCursorArg::Absent), &SMALL));
    assert_eq!(page.meta.generation, EMPTY_GENERATION);
    assert_eq!(world.calls("statistics_generation"), 0);
    assert!(run(&world, &request(1, bound(Some("x"), 0)), &SMALL).is_err());
    assert!(
        run(
            &world,
            &request(1, bound(Some(EMPTY_GENERATION), 1)),
            &SMALL
        )
        .is_err()
    );
}

#[test]
fn the_generation_covers_the_relevant_tasks_with_ancestors_that_are_running_by_identity_only() {
    let mut old = stat_facts("p", "running", 1_000_000, None);
    old.exit_code = None;
    let mut child = done("c", 0);
    child.continuation_of = Some(old.id.clone());
    let outside = stat_facts("z", "completed", 5_000_000, Some(6_000_000));
    let world = world(vec![child, old, outside]);
    ready(run(&world, &request(1, StatsCursorArg::Absent), &SMALL));
    let asked = world.feed.entries.borrow();
    assert_eq!(asked.len(), 1);
    let seen: Vec<(&str, bool)> = asked[0]
        .iter()
        .map(|e| (e.facts.id.as_str(), e.identity_only))
        .collect();
    assert_eq!(seen, [("c", false), ("p", true)]);
}

#[test]
fn the_generation_is_compared_before_the_offset_and_a_failure_to_digest_is_a_store_error() {
    let world = world(vec![done("a", 0), done("b", 1)]);
    let g = "g".repeat(64);
    let stale = run(&world, &request(1, bound(Some("stale"), 99)), &SMALL).unwrap_err();
    assert_eq!(
        stale,
        coded(
            "snapshot_expired",
            "Retained statistics history changed; restart the page walk"
        )
    );
    assert_eq!(
        run(&world, &request(1, bound(None, 0)), &SMALL).unwrap_err(),
        stale
    );
    for offset in [3, u64::MAX] {
        let error = run(&world, &request(1, bound(Some(&g), offset)), &SMALL).unwrap_err();
        assert_eq!(
            error,
            coded("invalid_cursor", "Statistics cursor order is invalid")
        );
    }
    let page = ready(run(&world, &request(5, bound(Some(&g), 2)), &SMALL));
    assert_eq!((page.items.len(), page.has_more), (0, false));
    let mut broken = self::world(vec![done("a", 0)]);
    broken.feed.generation = Err(StoreFailure::Busy);
    assert_eq!(
        run(&broken, &request(1, StatsCursorArg::Absent), &SMALL).unwrap_err(),
        ReadError::Store(StoreFailure::Busy)
    );
}

#[test]
fn an_expired_statistics_budget_ends_the_scan_whatever_the_cursor_says() {
    let mut slow = world((0..3).map(|n| done(&format!("t{n}"), n)).collect());
    slow.feed.batch_rows = 1;
    slow.feed.delay = Duration::from_millis(60);
    let reads = slow.reads(NONE_MARKERS);
    let budget = Duration::from_millis(100);
    let fresh = statistics_within(
        &reads,
        &root(),
        &request(1, StatsCursorArg::Absent),
        &SMALL,
        budget,
    );
    // Some rows were read before the budget ran out: the page is preparing.
    assert_eq!(fresh.unwrap(), StatsOut::Preparing(Vec::new()));
    slow.feed.batches.set(0);
    let walk = statistics_within(
        &reads,
        &root(),
        &request(1, bound(Some("g"), 0)),
        &SMALL,
        budget,
    );
    assert_eq!(
        walk.unwrap_err(),
        coded(
            "snapshot_expired",
            "Statistics source coverage changed; restart the page walk"
        )
    );
}

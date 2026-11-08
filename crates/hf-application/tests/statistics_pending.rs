#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The statistics-page budget in the scan: `preparing` once some rows are read, an unavailable
//! page when none is, a cursor walk expired either way, and the raw-history cap that precedes the scan.
mod statistics_support;
mod task_reads_support;

use hf_application::ctl::task_reads::{
    ReadError, ScanRow, StatsCursorArg, StatsOut, statistics_within,
};
use statistics_support::*;
use std::time::Duration;
use task_reads_support::{NONE_MARKERS, World, root, row as row_of, stat_facts};

fn within(
    world: &World,
    request: &hf_application::ctl::task_reads::StatsRequest,
    budget: Duration,
) -> Result<StatsOut, ReadError> {
    statistics_within(&world.reads(NONE_MARKERS), &root(), request, &SMALL, budget)
}

fn slow(rows: Vec<ScanRow>, batch_rows: usize) -> World {
    let mut world = World::new();
    world.feed.rows = rows;
    world.feed.batch_rows = batch_rows;
    world.feed.delay = Duration::from_millis(60);
    world
}

fn finished(count: usize) -> Vec<ScanRow> {
    (0..count)
        .map(|n| ScanRow::Facts(Box::new(done(&format!("t{n:03}"), n as i64))))
        .collect()
}

fn coverage() -> ReadError {
    coded(
        "snapshot_expired",
        "Statistics source coverage changed; restart the page walk",
    )
}

#[test]
fn a_budget_that_runs_out_after_some_rows_leaves_the_page_preparing() {
    let world = slow(finished(5), 1);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_millis(100),
    )
    .unwrap();
    assert_eq!(out, StatsOut::Preparing(Vec::new()));
    assert!(
        world.feed.batches.get() >= 1 && world.feed.batches.get() < 5,
        "{}",
        world.feed.batches.get()
    );
}

#[test]
fn preparing_lists_only_the_failures_of_the_rows_read_and_never_the_rows_still_pending() {
    let mut rows = vec![
        ScanRow::Oversized("a".into()),
        row_of(stat_facts("b", "bogus", FIRST, None)),
        ScanRow::Facts(Box::new(done("c", 0))),
    ];
    rows.extend(finished(1).into_iter().map(|r| match r {
        ScanRow::Facts(mut f) => {
            f.id = hf_domain::task_read::TaskId::new("z-pending".into()).unwrap();
            ScanRow::Facts(f)
        }
        other => other,
    }));
    let world = slow(rows, 3);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_millis(100),
    )
    .unwrap();
    let StatsOut::Preparing(found) = out else {
        panic!("{out:?}")
    };
    let seen: Vec<(&str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("source_oversized", "task:a"),
            ("source_unavailable", "task:b")
        ]
    );
}

#[test]
fn preparing_diagnostics_stop_at_sixteen() {
    let rows: Vec<ScanRow> = (0..40)
        .map(|n| ScanRow::Oversized(format!("o{n:02}")))
        .collect();
    let world = slow(rows, 20);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_millis(100),
    )
    .unwrap();
    let StatsOut::Preparing(found) = out else {
        panic!("{out:?}")
    };
    assert_eq!(found.len(), 16);
    assert_eq!(found[0].message, "task:o00");
    assert_eq!(found[15].message, "task:o15");
    assert!(found.iter().all(|d| d.code == "source_oversized"));
}

#[test]
fn a_budget_that_runs_out_before_any_row_is_unavailable_with_its_own_message() {
    let world = slow(finished(3), 1);
    let out = within(&world, &request(1, StatsCursorArg::Absent), Duration::ZERO).unwrap();
    assert_eq!(
        unavailable(Ok(out)),
        diag(
            "discovery_budget",
            "No source classification fits the foreground deadline"
        )
    );
    assert_eq!(world.calls("statistics_batch"), 0);
}

#[test]
fn a_cursor_walk_is_expired_whether_the_scan_made_progress_or_not() {
    for budget in [Duration::ZERO, Duration::from_millis(100)] {
        let world = slow(finished(5), 1);
        let walking = request(1, bound(Some("g"), 0));
        assert_eq!(
            within(&world, &walking, budget).unwrap_err(),
            coverage(),
            "{budget:?}"
        );
    }
}

#[test]
fn the_raw_history_cap_is_100000_rows_of_any_validity_and_is_judged_before_the_scan() {
    let mut world = slow(finished(3), 200);
    world.feed.delay = Duration::ZERO;
    world.feed.history_rows = Some(100_000);
    assert!(matches!(
        within(
            &world,
            &request(1, StatsCursorArg::Absent),
            Duration::from_secs(5)
        )
        .unwrap(),
        StatsOut::Ready(_)
    ));
    world.feed.history_rows = Some(100_001);
    world.feed.batches.set(0);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert_eq!(
        unavailable(out),
        diag("history_limit", "Raw history name limit exceeded")
    );
    assert_eq!(world.feed.batches.get(), 0, "no batch was read");
    let walking = request(1, bound(Some("g"), 0));
    assert_eq!(
        within(&world, &walking, Duration::from_secs(5)).unwrap_err(),
        coverage()
    );
}

#[test]
fn the_10000_source_limit_stays_beside_the_raw_cap() {
    let mut world = World::new();
    world.feed.rows = (0..10_001)
        .map(|n| ScanRow::Facts(Box::new(done(&format!("t{n:05}"), 0))))
        .collect();
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(30),
    );
    assert_eq!(
        unavailable(out),
        diag("history_limit", "Statistics source history limit exceeded")
    );
}

fn failing(count: usize) -> Vec<ScanRow> {
    (0..count)
        .map(|n| {
            if n % 2 == 0 {
                ScanRow::Oversized(format!("t{n:03}"))
            } else {
                ScanRow::Facts(Box::new(done(&format!("t{n:03}"), n as i64)))
            }
        })
        .collect()
}

fn seen(out: Result<StatsOut, ReadError>) -> Vec<String> {
    let StatsOut::Preparing(found) = out.unwrap() else {
        panic!("not preparing")
    };
    found.into_iter().map(|d| d.message).collect()
}

#[test]
fn the_rows_read_before_the_deadline_in_a_batch_are_kept_and_classified() {
    for (kept, wanted) in [(1usize, 0), (3, 1), (0, 0)] {
        let mut world = World::new();
        world.feed.rows = failing(10);
        world.feed.batch_rows = 5;
        world.feed.expire = Some((1, kept));
        let out = within(
            &world,
            &request(1, StatsCursorArg::Absent),
            Duration::from_secs(5),
        );
        // The first batch of five was read whole; the second kept `kept` rows and reported expiry.
        let _ = wanted;
        let found = seen(out);
        let mut expected = vec![
            "task:t000".to_string(),
            "task:t002".into(),
            "task:t004".into(),
        ];
        expected.extend(
            (5..5 + kept)
                .filter(|n| n % 2 == 0)
                .map(|n| format!("task:t{n:03}")),
        );
        assert_eq!(found, expected, "kept {kept}");
    }
}

#[test]
fn an_expired_batch_never_reads_as_the_end_of_the_scan() {
    // Every row of the world was read, but the batch said it expired: the page is not ready.
    let mut world = World::new();
    world.feed.rows = finished(4);
    world.feed.batch_rows = 200;
    world.feed.expire = Some((0, 4));
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert_eq!(seen(out), Vec::<String>::new());
    let walking = request(1, bound(Some("g"), 0));
    world.feed.batches.set(0);
    assert_eq!(
        within(&world, &walking, Duration::from_secs(5)).unwrap_err(),
        coverage()
    );
}

#[test]
fn an_expired_first_batch_without_rows_is_the_no_progress_page_and_a_later_one_is_preparing() {
    let mut world = World::new();
    world.feed.rows = finished(6);
    world.feed.batch_rows = 3;
    world.feed.expire = Some((0, 0));
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert_eq!(
        unavailable(out),
        diag(
            "discovery_budget",
            "No source classification fits the foreground deadline"
        )
    );
    world.feed.batches.set(0);
    world.feed.expire = Some((1, 0));
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert_eq!(seen(out), Vec::<String>::new());
}

#[test]
fn a_budget_that_overflows_the_clock_is_no_panic() {
    let mut world = World::new();
    world.feed.rows = finished(3);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_millis(u64::MAX),
    );
    assert!(matches!(out.unwrap(), StatsOut::Ready(_)));
    let out = within(&world, &request(1, StatsCursorArg::Absent), Duration::MAX);
    assert!(matches!(out.unwrap(), StatsOut::Ready(_)));
}

#[test]
fn the_10000_valid_row_count_is_judged_before_the_scan_reads_a_batch() {
    let mut world = World::new();
    world.feed.rows = finished(3);
    world.feed.valid_rows = Some(10_000);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert!(matches!(out.unwrap(), StatsOut::Ready(_)));
    world.feed.valid_rows = Some(10_001);
    world.feed.batches.set(0);
    let out = within(
        &world,
        &request(1, StatsCursorArg::Absent),
        Duration::from_secs(5),
    );
    assert_eq!(
        unavailable(out),
        diag("history_limit", "Statistics source history limit exceeded")
    );
    assert_eq!(world.feed.batches.get(), 0, "no batch was read");
    let walking = request(1, bound(Some("g"), 0));
    assert_eq!(
        within(&world, &walking, Duration::from_secs(5)).unwrap_err(),
        coverage()
    );
}

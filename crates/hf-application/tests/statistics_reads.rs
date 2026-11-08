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
fn markers_are_judged_before_the_root_and_by_presence() {
    for class in [RootClass::Absent, RootClass::MarkedStore] {
        let mut world = world(vec![done("a", 0)]);
        world.class = class;
        for markers in [
            EMPTY_MARKER,
            EnvMarkers {
                present: true,
                non_empty: true,
            },
        ] {
            let result = statistics(
                &world.reads(markers),
                &root(),
                &request(1, StatsCursorArg::Absent),
                &SMALL,
            );
            assert!(
                matches!(result, Err(ReadError::Refused(RefusalKind::Agent))),
                "{markers:?}"
            );
            assert_eq!(world.calls("classify"), 0);
        }
    }
}

#[test]
fn parameters_come_before_the_cursor_and_the_cursor_before_the_binding() {
    let world = world(vec![done("a", 0)]);
    let params = coded("invalid_params", "x");
    let message = "Statistics require ordered timezone-qualified bounds and limit 1..100";
    let expected = ReadError::InvalidParams(message.into());
    let _ = params;
    for limit in [0, 101, -1, i64::MAX] {
        assert_eq!(
            run(&world, &request(limit, StatsCursorArg::Malformed), &SMALL).unwrap_err(),
            expected
        );
    }
    let none = StatsRequest {
        bounds: None,
        limit: 1,
        cursor: StatsCursorArg::Foreign,
    };
    assert_eq!(run(&world, &none, &SMALL).unwrap_err(), expected);
    assert_eq!(
        run(&world, &request(1, StatsCursorArg::Malformed), &SMALL).unwrap_err(),
        coded("invalid_cursor", "Invalid statistics cursor")
    );
    assert_eq!(
        run(&world, &request(1, StatsCursorArg::Foreign), &SMALL).unwrap_err(),
        coded(
            "invalid_cursor",
            "Statistics cursor belongs to another query or state root"
        )
    );
    assert_eq!(world.calls("statistics_batch"), 0);
}

#[test]
fn a_missing_root_runs_the_same_checks_and_answers_the_empty_page() {
    let mut world = world(vec![done("a", 0)]);
    world.class = RootClass::Absent;
    let page = ready(run(&world, &request(1, StatsCursorArg::Absent), &SMALL));
    assert_eq!(
        (
            page.meta.generation.as_str(),
            page.meta.eligible,
            page.items.len()
        ),
        (EMPTY_GENERATION, 0, 0)
    );
    assert_eq!(world.calls("snapshot"), 0);
    let stale = request(1, bound(Some("x"), 0));
    assert_eq!(
        run(&world, &stale, &SMALL).unwrap_err(),
        coded(
            "snapshot_expired",
            "Retained statistics history changed; restart the page walk"
        )
    );
    let beyond = request(1, bound(Some(EMPTY_GENERATION), 1));
    assert_eq!(
        run(&world, &beyond, &SMALL).unwrap_err(),
        coded("invalid_cursor", "Statistics cursor order is invalid")
    );
    assert!(
        run(
            &world,
            &request(1, bound(Some(EMPTY_GENERATION), 0)),
            &SMALL
        )
        .is_ok()
    );
    assert!(run(&world, &request(1, bound(None, 0)), &SMALL).is_err());
    assert!(run(&world, &request(0, StatsCursorArg::Absent), &SMALL).is_err());
}

#[test]
fn items_are_the_finished_tasks_whose_start_is_in_the_window() {
    let mut early = done("a", 0);
    early.started_at_us = 1_767_225_599_999_999;
    let mut at_since = done("b", 0);
    at_since.started_at_us = 1_767_225_600_000_000;
    at_since.finished_at_us = Some(1_767_225_601_000_000);
    let mut at_before = done("c", 0);
    at_before.started_at_us = 1_798_761_600_000_000;
    at_before.finished_at_us = Some(1_798_761_601_000_000);
    let mut running = stat_facts("d", "running", FIRST, None);
    running.exit_code = None;
    let cancelled = stat_facts("e", "cancelled", FIRST + 1, Some(FIRST + 2));
    let timed = stat_facts("f", "timed_out", FIRST + 3, Some(FIRST + 4));
    let page = ready(run(
        &world(vec![early, at_since, at_before, running, cancelled, timed]),
        &request(100, StatsCursorArg::Absent),
        &SMALL,
    ));
    let ids: Vec<&str> = page.items.iter().map(|i| i.task.as_str()).collect();
    assert_eq!(ids, ["b", "e", "f"]);
    assert_eq!(page.meta.eligible, 3);
}

#[test]
fn items_sort_by_the_start_as_a_float_then_the_id() {
    // Beyond 2**53 microseconds two instants one microsecond apart are the same float.
    let late = 253_402_257_600_000_000;
    let mut z = done("z1", 0);
    z.started_at_us = late + 1;
    z.finished_at_us = Some(late + 3_600_000_000);
    let mut a = done("a1", 0);
    a.started_at_us = late + 2;
    a.finished_at_us = Some(late + 3_600_000_000);
    let wide = StatsRequest {
        bounds: bounds("9999-12-31T00:00:00Z", "9999-12-31T20:00:00Z"),
        limit: 100,
        cursor: StatsCursorArg::Absent,
    };
    let page = ready(run(&world(vec![a, z]), &wide, &SMALL));
    assert_eq!(page.items[0].started_at, page.items[1].started_at);
    let ids: Vec<&str> = page.items.iter().map(|i| i.task.as_str()).collect();
    assert_eq!(ids, ["a1", "z1"]);
    let mut earlier = done("m", 0);
    earlier.started_at_us = FIRST;
    let page = ready(run(
        &world(vec![done("b", 5), done("a", 5), earlier]),
        &request(100, StatsCursorArg::Absent),
        &SMALL,
    ));
    let ids: Vec<&str> = page.items.iter().map(|i| i.task.as_str()).collect();
    assert_eq!(ids, ["m", "a", "b"]);
}

#[test]
fn elapsed_is_the_difference_of_the_two_floats() {
    let mut fact = done("a", 0);
    fact.started_at_us = 1_768_039_200_000_001;
    fact.finished_at_us = Some(1_768_039_200_000_002);
    let page = ready(run(
        &world(vec![fact]),
        &request(1, StatsCursorArg::Absent),
        &SMALL,
    ));
    assert_eq!(page.items[0].elapsed_seconds, 9.5367431640625e-07);
}

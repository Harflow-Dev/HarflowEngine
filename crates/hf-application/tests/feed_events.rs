#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The interaction-events use case over scripted ports: the gate and its order, the candidate
//! rules, the 25-source page, the payload and metadata bounds, and resumption from a cursor.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        FeedCursorArg, FeedPage, FeedRequest, ReadError, StoreFailure, interaction_events,
    },
};
use hf_domain::ctl::{
    caller::{EnvMarkers, PidObservation, RefusalKind},
    feed::{FeedEvent, FeedSource, FeedState, METADATA_LIMIT, PAYLOAD_LIMIT, Phase, candidate},
};
use task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, World, caller, identity, root, source,
};

fn done(id: &str) -> FeedSource {
    source(id, "completed", Some(0))
}

fn world(ids: &[&str]) -> World {
    let mut world = World::new();
    world.feed.sources = ids.iter().map(|id| done(id)).collect();
    world
}

fn request(limit: i64, cursor: FeedCursorArg) -> FeedRequest {
    FeedRequest { limit, cursor }
}

fn plain() -> FeedRequest {
    request(100, FeedCursorArg::Absent)
}

fn small(events: &[FeedEvent]) -> usize {
    events.len() * 100
}

fn run(world: &World, markers: EnvMarkers, request: &FeedRequest) -> Result<FeedPage, ReadError> {
    interaction_events(&world.reads(markers), &root(), request, &small)
}

fn ids(page: &FeedPage) -> Vec<String> {
    page.events
        .iter()
        .map(|e| e.task.as_str().to_string())
        .collect()
}

fn state(phase: Phase, after: &str, source: &str, event: &str) -> FeedState {
    FeedState {
        phase,
        after: after.into(),
        source: source.into(),
        event: event.into(),
    }
}

fn names(count: usize) -> Vec<String> {
    (0..count).map(|n| format!("t{n:02}")).collect()
}

#[test]
fn only_completed_and_failed_tasks_with_an_exit_code_yield_events() {
    let mut world = World::new();
    world.feed.sources = vec![
        source("a1", "completed", Some(0)),
        source("a2", "failed", Some(2)),
        source("a3", "timed_out", None),
        source("a4", "cancelled", None),
        source("a5", "cancelled", Some(0)),
        source("a6", "timed_out", Some(1)),
        source("a7", "completed", None),
        source("a8", "failed", None),
        source("a9", "running", None),
        source("b1", "running", Some(0)),
        source("b2", "bogus", Some(0)),
    ];
    let page = run(&world, NONE_MARKERS, &plain()).unwrap();
    assert_eq!(ids(&page), ["a1", "a2"]);
    assert_eq!((page.next, page.gaps.len()), (None, 0));
}

#[test]
fn a_title_is_the_id_when_missing_or_empty_and_is_cut_at_200_code_points() {
    let mut found = done("x1");
    let mut title = |text: Option<&str>| {
        found.title = text.map(str::to_string);
        candidate(&found).unwrap().title
    };
    assert_eq!(title(None), "x1");
    assert_eq!(title(Some("")), "x1");
    assert_eq!(title(Some("é")), "é");
    assert_eq!(title(Some(&"x".repeat(200))).chars().count(), 200);
    let cut = title(Some(&"\u{1f600}".repeat(201)));
    assert_eq!(cut.chars().count(), 200);
    assert_eq!(cut.len(), 800);
}

#[test]
fn the_time_is_the_finish_else_the_start_as_seconds() {
    let mut found = done("x1");
    found.started_at_us = 1_500_000;
    found.finished_at_us = Some(2_250_000);
    assert_eq!(candidate(&found).unwrap().occurred_at, 2.25);
    found.finished_at_us = None;
    assert_eq!(candidate(&found).unwrap().occurred_at, 1.5);
    found.started_at_us = -1;
    assert_eq!(candidate(&found).unwrap().occurred_at, -1e-6);
    found.started_at_us = 253_402_300_799_999_999;
    assert_eq!(candidate(&found).unwrap().occurred_at, 253_402_300_800.0);
}

#[test]
fn a_page_reads_25_sources_and_names_the_last_as_after() {
    let list = names(30);
    let refs: Vec<&str> = list.iter().map(String::as_str).collect();
    let world = world(&refs);
    let page = run(&world, NONE_MARKERS, &plain()).unwrap();
    assert_eq!(page.events.len(), 25);
    assert_eq!(page.next, Some(state(Phase::Task, "t24", "", "")));
    let rest = run(
        &world,
        NONE_MARKERS,
        &request(100, FeedCursorArg::State(page.next.unwrap())),
    )
    .unwrap();
    assert_eq!(ids(&rest), ["t25", "t26", "t27", "t28", "t29"]);
    assert_eq!(rest.next, None);
}

#[test]
fn exactly_25_sources_still_end_the_page_with_a_cursor() {
    let list = names(25);
    let refs: Vec<&str> = list.iter().map(String::as_str).collect();
    let page = run(&world(&refs), NONE_MARKERS, &plain()).unwrap();
    assert_eq!(page.events.len(), 25);
    assert_eq!(page.next, Some(state(Phase::Task, "t24", "", "")));
    let list = names(24);
    let refs: Vec<&str> = list.iter().map(String::as_str).collect();
    let page = run(&world(&refs), NONE_MARKERS, &plain()).unwrap();
    assert_eq!((page.events.len(), page.next), (24, None));
}

#[test]
fn running_sources_count_toward_the_25() {
    let mut world = World::new();
    world.feed.sources = (0..30)
        .map(|n| source(&format!("t{n:02}"), "running", None))
        .collect();
    let page = run(&world, NONE_MARKERS, &plain()).unwrap();
    assert!(page.events.is_empty());
    assert_eq!(page.next, Some(state(Phase::Task, "t24", "", "")));
}

#[test]
fn a_full_page_names_the_source_it_stopped_before() {
    let world = world(&["a", "b", "c", "d"]);
    let page = run(&world, NONE_MARKERS, &request(2, FeedCursorArg::Absent)).unwrap();
    assert_eq!(ids(&page), ["a", "b"]);
    assert_eq!(page.next, Some(state(Phase::Task, "b", "c", "")));
    let next = run(
        &world,
        NONE_MARKERS,
        &request(2, FeedCursorArg::State(page.next.unwrap())),
    )
    .unwrap();
    assert_eq!(ids(&next), ["c", "d"]);
    assert_eq!(next.next, None);
}

#[test]
fn a_page_exactly_at_the_limit_completes_when_nothing_is_left() {
    let world = world(&["a", "b"]);
    let page = run(&world, NONE_MARKERS, &request(2, FeedCursorArg::Absent)).unwrap();
    assert_eq!(
        (ids(&page), page.next),
        (vec!["a".into(), "b".into()], None)
    );
}

#[test]
fn the_payload_bound_is_131072_bytes_and_a_byte_more_ends_the_page() {
    let world = world(&["a", "b", "c", "d"]);
    for (edge, wanted) in [
        (PAYLOAD_LIMIT, 3),
        (PAYLOAD_LIMIT - 1, 3),
        (PAYLOAD_LIMIT + 1, 2),
    ] {
        // The third event is the one that crosses: its list measures `edge`.
        let size = move |events: &[FeedEvent]| {
            if events.len() < 3 {
                10
            } else if events.len() == 3 {
                edge
            } else {
                PAYLOAD_LIMIT + 1
            }
        };
        let page =
            interaction_events(&world.reads(NONE_MARKERS), &root(), &plain(), &size).unwrap();
        assert_eq!(page.events.len(), wanted, "edge {edge}");
        assert!(page.next.is_some());
    }
    assert_eq!(PAYLOAD_LIMIT, 131_072);
}

#[test]
fn an_event_that_alone_passes_the_payload_bound_gives_an_empty_page_that_names_its_source() {
    let world = world(&["a", "b"]);
    let size = |_: &[FeedEvent]| PAYLOAD_LIMIT + 1;
    let page = interaction_events(&world.reads(NONE_MARKERS), &root(), &plain(), &size).unwrap();
    assert!(page.events.is_empty());
    assert_eq!(page.next, Some(state(Phase::Task, "", "a", "")));
}

fn heavy(sizes: &[u64]) -> World {
    let mut world = World::new();
    world.feed.sources = sizes
        .iter()
        .enumerate()
        .map(|(n, size)| FeedSource {
            metadata_bytes: *size,
            ..done(&format!("m{n}"))
        })
        .collect();
    world
}

#[test]
fn the_metadata_budget_is_8_mib_exact() {
    let half = METADATA_LIMIT / 2;
    let page = run(&heavy(&[half, half, 1]), NONE_MARKERS, &plain()).unwrap();
    assert_eq!(ids(&page), ["m0", "m1"]);
    assert_eq!(page.next, Some(state(Phase::Task, "m1", "", "")));
    let page = run(&heavy(&[half, half + 1, 1]), NONE_MARKERS, &plain()).unwrap();
    assert_eq!(ids(&page), ["m0"]);
    assert_eq!(page.next, Some(state(Phase::Task, "m0", "", "")));
    assert_eq!(METADATA_LIMIT, 8 * 1024 * 1024);
}

#[test]
fn the_first_source_of_a_page_is_read_whatever_its_size() {
    let page = run(&heavy(&[METADATA_LIMIT + 1, 1]), NONE_MARKERS, &plain()).unwrap();
    assert_eq!(ids(&page), ["m0"]);
    assert_eq!(page.next, Some(state(Phase::Task, "m0", "", "")));
}

#[test]
fn a_source_below_after_is_read_and_the_walk_resumes_after_it() {
    let world = world(&["a", "b", "c"]);
    let cursor = FeedCursorArg::State(state(Phase::Task, "c", "a", ""));
    let page = run(&world, NONE_MARKERS, &request(100, cursor)).unwrap();
    assert_eq!(ids(&page), ["a", "b", "c"]);
}

#[test]
fn an_absent_task_source_and_a_workflow_source_are_gaps() {
    let world = world(&["a", "c"]);
    let cursor = FeedCursorArg::State(state(Phase::Task, "", "b", ""));
    let page = run(&world, NONE_MARKERS, &request(100, cursor)).unwrap();
    assert_eq!(ids(&page), ["c"]);
    assert_eq!(page.gaps.len(), 1);
    assert_eq!(
        (page.gaps[0].source.as_str(), page.gaps[0].reason),
        ("b", "source_unavailable")
    );
    let cursor = FeedCursorArg::State(state(Phase::Workflow, "", "run1", ""));
    let page = run(&world, NONE_MARKERS, &request(100, cursor)).unwrap();
    assert_eq!(ids(&page), ["a", "c"]);
    assert_eq!(
        (page.gaps[0].source.as_str(), page.gaps[0].reason),
        ("run1", "source_unavailable")
    );
    assert_eq!(
        world.calls("feed_source:run1"),
        0,
        "a workflow source is not looked up as a task"
    );
}

#[test]
fn a_workflow_phase_without_a_source_goes_straight_to_the_tasks() {
    let world = world(&["a"]);
    let cursor = FeedCursorArg::State(state(Phase::Workflow, "run9", "", ""));
    let page = run(&world, NONE_MARKERS, &request(100, cursor)).unwrap();
    assert_eq!((ids(&page), page.gaps.len()), (vec!["a".into()], 0));
    assert_eq!(
        *world.feed.asked.borrow(),
        [(String::new(), 1), ("a".into(), 1)]
    );
}

#[test]
fn a_cursor_event_at_or_above_the_key_skips_the_event() {
    let world = world(&["a"]);
    let key = "00000000000000002.000000:1:task:a:completed";
    for (event, wanted) in [
        (key, 0),
        ("00000000000000002.000000:1:task:a:complete", 1),
        ("9", 0),
        ("", 1),
    ] {
        let cursor = FeedCursorArg::State(state(Phase::Task, "", "a", event));
        let page = run(&world, NONE_MARKERS, &request(100, cursor)).unwrap();
        assert_eq!(page.events.len(), wanted, "{event}");
    }
}

#[test]
fn the_cursor_is_judged_before_the_limit_and_both_after_the_gate() {
    let world = world(&["a"]);
    let bad = |limit, cursor| run(&world, NONE_MARKERS, &request(limit, cursor));
    let cursor_error = ReadError::Coded {
        code: "interaction_events_error",
        message: "Invalid interaction-events cursor".into(),
    };
    let limit_error = ReadError::Coded {
        code: "interaction_events_error",
        message: "limit must be between 1 and 100".into(),
    };
    assert_eq!(bad(0, FeedCursorArg::Malformed), Err(cursor_error));
    assert_eq!(bad(0, FeedCursorArg::Absent), Err(limit_error.clone()));
    assert_eq!(bad(101, FeedCursorArg::Absent), Err(limit_error.clone()));
    assert_eq!(bad(i64::MIN, FeedCursorArg::Absent), Err(limit_error));
    assert!(bad(1, FeedCursorArg::Absent).is_ok());
    assert!(bad(100, FeedCursorArg::Absent).is_ok());
}

#[test]
fn a_missing_root_validates_and_answers_empty_without_a_snapshot() {
    for class in [RootClass::Absent, RootClass::Empty] {
        let mut world = world(&["a"]);
        world.class = class;
        let page = run(&world, NONE_MARKERS, &plain()).unwrap();
        assert_eq!(
            (page.events.len(), page.next, page.gaps.len()),
            (0, None, 0)
        );
        assert_eq!(world.calls("snapshot"), 0);
        assert!(run(&world, NONE_MARKERS, &request(0, FeedCursorArg::Absent)).is_err());
        assert!(run(&world, NONE_MARKERS, &request(1, FeedCursorArg::Malformed)).is_err());
    }
}

fn refused(kind: RefusalKind) -> Result<FeedPage, ReadError> {
    Err(ReadError::Refused(kind))
}

#[test]
fn a_marker_refuses_by_presence_after_the_root_is_classified_and_before_any_feed_query() {
    let world = world(&["a"]);
    for markers in [EMPTY_MARKER, FULL_MARKER] {
        let result = run(&world, markers, &plain());
        assert!(matches!(result, Err(ReadError::Refused(_))), "{markers:?}");
    }
    assert_eq!(world.calls("feed_source"), 0);
    assert_eq!(world.order().first().map(String::as_str), Some("classify"));
}

#[test]
fn a_foreign_root_is_a_state_error_even_with_a_marker() {
    let mut world = world(&["a"]);
    let found = hf_domain::ctl::inspect::StateInspection::new(
        &root(),
        hf_domain::ctl::inspect::InspectStatus::Incompatible,
        hf_domain::ctl::inspect::InspectReason::UnmarkedNonempty,
        vec![],
    );
    world.class = RootClass::Refused(found);
    for markers in [NONE_MARKERS, EMPTY_MARKER, FULL_MARKER] {
        assert!(
            matches!(run(&world, markers, &plain()), Err(ReadError::State(_))),
            "{markers:?}"
        );
    }
}

#[test]
fn an_alive_ancestor_row_is_an_agent_and_an_unreadable_one_is_undecidable() {
    let mut world = world(&["a"]);
    world.callers = vec![caller(9, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    assert_eq!(
        run(&world, NONE_MARKERS, &plain()),
        refused(RefusalKind::Agent)
    );
    world.seen.insert(400, PidObservation::Unknown);
    assert_eq!(
        run(&world, NONE_MARKERS, &plain()),
        refused(RefusalKind::Undecidable)
    );
    world.seen.insert(400, PidObservation::Absent);
    assert!(run(&world, NONE_MARKERS, &plain()).is_ok());
}

#[test]
fn damage_is_judged_first_inside_the_snapshot_and_reads_no_feed() {
    let mut world = world(&["a"]);
    world.damaged = true;
    assert_eq!(run(&world, NONE_MARKERS, &plain()), Err(ReadError::Damaged));
    assert_eq!(world.calls("feed_source"), 0);
    assert_eq!(world.calls("related"), 0);
}

#[test]
fn a_busy_store_is_reported_as_such() {
    let mut world = world(&["a"]);
    world.open_error = Some(StoreFailure::Busy);
    assert_eq!(
        run(&world, NONE_MARKERS, &plain()),
        Err(ReadError::Store(StoreFailure::Busy))
    );
}

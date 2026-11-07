#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The event page read over scripted ports: the direct-only gate and its order, cursor
//! classification and error mapping.
mod task_history_support;
mod task_reads_support;
use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{EventCursorArg, EventFailure, ReadError, Resolution, StoreFailure, events},
};
use hf_domain::{
    ctl::{
        caller::{PidObservation, RefusalKind},
        inspect::InspectReason,
    },
    event_read::{CaptureState, EventFilter},
};
use task_history_support::*;
use task_reads_support::*;

#[test]
fn the_order_is_gate_then_resolution_then_page_then_record_then_token() {
    let world = world();
    run(&world, NONE_MARKERS, EventCursorArg::Absent).unwrap();
    assert_eq!(
        world.order(),
        [
            "classify",
            "view",
            "snapshot",
            "damaged",
            "related",
            "lookup",
            "event_page",
            "record",
            "event_change_token"
        ]
    );
}

#[test]
fn any_present_marker_refuses_before_the_root_is_looked_at_even_when_empty() {
    for markers in [FULL_MARKER, EMPTY_MARKER] {
        let world = world();
        let outcome = run(&world, markers, EventCursorArg::Absent);
        assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
        assert!(world.order().is_empty());
    }
}

#[test]
fn an_agent_is_refused_before_any_task_is_read() {
    let mut world = world();
    world.callers = vec![caller(9, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    let outcome = run(&world, NONE_MARKERS, EventCursorArg::Absent);
    assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
    for name in [
        "lookup",
        "record",
        "event_page",
        "event_change_token",
        "foreign_binds",
    ] {
        assert_eq!(world.calls(name), 0, "{name}");
    }
}

#[test]
fn undecidable_callers_and_damage_are_refused_without_a_task_read() {
    let mut world = world();
    world.callers = vec![caller(9, 400)];
    world.seen.insert(400, PidObservation::Unknown);
    let outcome = run(&world, NONE_MARKERS, EventCursorArg::Absent);
    assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Undecidable)));
    let mut damaged = self::world();
    damaged.damaged = true;
    let outcome = run(&damaged, NONE_MARKERS, EventCursorArg::Absent);
    assert_eq!(
        outcome,
        Err(ReadError::Damaged),
        "the transport words it as an undecidable caller"
    );
    assert_eq!(damaged.calls("lookup"), 0);
}

#[test]
fn a_root_without_a_store_is_an_unknown_task_and_opens_nothing() {
    for class in [RootClass::Absent, RootClass::Empty] {
        let mut world = world();
        world.class = class;
        let outcome = run(&world, NONE_MARKERS, EventCursorArg::Malformed);
        assert!(matches!(
            outcome,
            Err(ReadError::Resolution(Resolution::Unknown {
                reason: None,
                ..
            }))
        ));
        assert_eq!(world.order(), ["classify"]);
    }
}

#[test]
fn an_unknown_task_wins_over_a_bad_cursor() {
    let mut world = World::new();
    world.tasks.push(record(2, "completed", Some(0), None));
    for cursor in [
        EventCursorArg::Malformed,
        EventCursorArg::Native(native(EventFilter::Default.kinds(), 1)),
        EventCursorArg::Foreign {
            task: "t".into(),
            filter: "f".into(),
        },
    ] {
        let outcome = run(&world, NONE_MARKERS, cursor);
        assert!(
            matches!(outcome, Err(ReadError::Resolution(_))),
            "{outcome:?}"
        );
    }
    assert_eq!(world.calls("event_page"), 0);
    assert_eq!(world.calls("foreign_binds"), 0);
}

#[test]
fn a_malformed_cursor_is_invalid_and_reads_no_page() {
    let world = world();
    let outcome = run(&world, NONE_MARKERS, EventCursorArg::Malformed);
    assert_eq!(outcome, Err(ReadError::CursorInvalid));
    assert_eq!(world.calls("event_page"), 0);
}

#[test]
fn a_native_cursor_must_name_this_task_and_this_filter_and_be_well_formed() {
    let default = EventFilter::Default.kinds();
    let mut bad_store = native(default.clone(), 1);
    bad_store.store = "short".into();
    let mut bad_generation = native(default.clone(), 1);
    bad_generation.generation = "g".repeat(32);
    let mut bad_boundary = native(default.clone(), 1);
    bad_boundary.boundary = u64::MAX;
    for (label, cursor) in [
        ("other task", native(default.clone(), 2)),
        ("other kinds", native(vec!["notice".into()], 1)),
        (
            "unsorted kinds",
            native(vec!["usage".into(), "notice".into()], 1),
        ),
        ("empty kinds", native(Vec::new(), 1)),
        ("unknown kind", native(vec!["bogus".into()], 1)),
        ("store", bad_store),
        ("generation", bad_generation),
        ("boundary", bad_boundary),
    ] {
        let world = world();
        let outcome = run(&world, NONE_MARKERS, EventCursorArg::Native(cursor));
        assert_eq!(outcome, Err(ReadError::CursorInvalid), "{label}");
        assert_eq!(world.calls("event_page"), 0, "{label}");
    }
}

#[test]
fn a_matching_native_cursor_reaches_the_page_unchanged() {
    let world = world();
    let cursor = native(EventFilter::Default.kinds(), 1);
    run(&world, NONE_MARKERS, EventCursorArg::Native(cursor.clone())).unwrap();
    let seen = world.history.queries.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].cursor.as_ref(), Some(&cursor));
    assert_eq!((seen[0].limit, seen[0].from_oldest), (100, false));
}

#[test]
fn a_foreign_cursor_is_expired_when_it_binds_this_task_and_filter_and_invalid_otherwise() {
    for (binds, expected) in [
        (true, ReadError::CursorExpired),
        (false, ReadError::CursorInvalid),
    ] {
        let mut world = world();
        world.history.binds = binds;
        let outcome = run(
            &world,
            NONE_MARKERS,
            EventCursorArg::Foreign {
                task: "task-digest".into(),
                filter: "filter-digest".into(),
            },
        );
        assert_eq!(outcome, Err(expected));
        assert_eq!(
            world.calls("event_page"),
            0,
            "a foreign cursor never reads a page"
        );
        let asked = world.history.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].0, task_id(1).as_str());
        assert_eq!(asked[0].1, EventFilter::Default.kinds());
        assert_eq!(
            (asked[0].2.as_str(), asked[0].3.as_str()),
            ("task-digest", "filter-digest")
        );
    }
}

#[test]
fn a_foreign_cursor_is_asked_about_the_requested_kinds() {
    let world = world();
    let mut asked = request(EventCursorArg::Foreign {
        task: "a".into(),
        filter: "b".into(),
    });
    asked.filter = EventFilter::Kinds(vec!["usage".into(), "notice".into(), "usage".into()]);
    let _ = events(&world.reads(NONE_MARKERS), &root(), &asked, &|_| true);
    assert_eq!(world.history.asked.borrow()[0].1, ["notice", "usage"]);
}

#[test]
fn an_invalid_limit_is_a_parameter_error_and_not_a_cursor_code() {
    for limit in [0, 201] {
        let world = world();
        let mut bad = request(EventCursorArg::Absent);
        bad.limit = limit;
        let outcome = events(&world.reads(NONE_MARKERS), &root(), &bad, &|_| true);
        assert!(
            matches!(outcome, Err(ReadError::InvalidParams(_))),
            "{limit}: {outcome:?}"
        );
    }
}

#[test]
fn a_bad_stored_row_is_a_store_failure_with_or_without_a_cursor_never_a_cursor_code() {
    let failure = StoreFailure::Incompatible {
        reason: InspectReason::InfrastructureUnavailable,
        message: "Task history cannot be read".into(),
    };
    let make = || {
        let mut world = world();
        let failure = failure.clone();
        world.history.pages = Box::new(move |_| Err(EventFailure::Store(failure.clone())));
        world
    };
    for cursor in [
        EventCursorArg::Absent,
        EventCursorArg::Native(native(EventFilter::Default.kinds(), 1)),
    ] {
        let outcome = run(&make(), NONE_MARKERS, cursor);
        assert_eq!(outcome, Err(ReadError::Store(failure.clone())));
    }
}

#[test]
fn only_the_expiry_of_a_cursor_path_is_a_cursor_expiry() {
    let mut world = world();
    world.history.pages = Box::new(|_| Err(EventFailure::Expired));
    let cursor = EventCursorArg::Native(native(EventFilter::Default.kinds(), 1));
    assert_eq!(
        run(&world, NONE_MARKERS, cursor),
        Err(ReadError::CursorExpired)
    );
}

#[test]
fn the_log_object_comes_from_the_record_and_the_stream_after_the_page() {
    let mut world = world();
    world.history.pages = Box::new(|query| {
        let mut page = page_of(&query.task);
        page.facts = Some(log_facts(CaptureState::LostWithOwner));
        Ok(page)
    });
    let out = run(&world, NONE_MARKERS, EventCursorArg::Absent).unwrap();
    assert_eq!(
        out.log.capture,
        CaptureState::ObservedTerminal,
        "the record is authoritative"
    );
    assert_eq!(out.change_token, "0".repeat(64));
}

#[test]
fn a_page_without_log_facts_is_a_vanished_task_not_a_silent_answer() {
    let mut world = world();
    world.history.pages = Box::new(|query| {
        let mut page = page_of(&query.task);
        page.facts = None;
        Ok(page)
    });
    assert!(run(&world, NONE_MARKERS, EventCursorArg::Absent).is_err());
}

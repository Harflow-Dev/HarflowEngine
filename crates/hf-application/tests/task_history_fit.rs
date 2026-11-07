#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The fit of an event page to the document bound, and the prompt read, over scripted ports.
mod task_history_support;
mod task_reads_support;
use hf_application::ctl::task_reads::{
    EventCursorArg, EventFailure, ReadError, StoreFailure, events, prompt,
};
use hf_domain::{
    ctl::{caller::RefusalKind, history::PromptBlob},
    event_read::EventFilter,
};
use std::cell::Cell;
use task_history_support::*;
use task_reads_support::*;

fn counting(count: usize, fits_at: usize) -> (World, Cell<usize>) {
    let mut world = world();
    world.history.pages = Box::new(move |query| {
        let mut page = pages_of(count.min(query.limit));
        page.task = Some(query.task.clone());
        Ok(page)
    });
    let _ = fits_at;
    (world, Cell::new(0))
}

/// The document fits when its page holds at most `capacity` decoded lines.
fn shrink(count: usize, capacity: usize) -> (usize, usize) {
    let (world, calls) = counting(count, capacity);
    let mut asked = request(EventCursorArg::Absent);
    asked.limit = count;
    let out = events(&world.reads(NONE_MARKERS), &root(), &asked, &|out| {
        calls.set(calls.get() + 1);
        out.page.decoded <= capacity
    })
    .unwrap();
    (out.page.decoded, world.history.queries.borrow().len())
}

#[test]
fn a_page_that_fits_is_read_once() {
    assert_eq!(shrink(100, 100), (100, 1));
    assert_eq!(shrink(100, 500), (100, 1));
}

#[test]
fn an_oversized_page_is_read_again_for_the_largest_limit_that_fits() {
    for capacity in 1..100 {
        let (decoded, reads) = shrink(100, capacity);
        assert_eq!(decoded, capacity, "capacity {capacity}");
        assert!(reads <= 1 + 8, "capacity {capacity}: {reads} reads");
    }
}

#[test]
fn the_search_covers_two_hundred_lines_in_at_most_eight_reruns() {
    for capacity in [1, 2, 3, 57, 100, 128, 199] {
        let (decoded, reads) = shrink(200, capacity);
        assert_eq!(decoded, capacity, "capacity {capacity}");
        assert!(reads <= 9, "capacity {capacity}: {reads} reads");
    }
}

#[test]
fn a_line_that_alone_exceeds_the_bound_is_kept_and_the_search_ends() {
    let (world, _) = counting(100, 0);
    let out = events(
        &world.reads(NONE_MARKERS),
        &root(),
        &request(EventCursorArg::Absent),
        &|_| false,
    )
    .unwrap();
    assert_eq!(out.page.decoded, 1, "limit 1 always returns");
    let limits: Vec<usize> = world
        .history
        .queries
        .borrow()
        .iter()
        .map(|q| q.limit)
        .collect();
    assert_eq!(limits.first(), Some(&100));
    assert_eq!(limits.last(), Some(&1));
    assert!(limits.len() <= 9, "{limits:?}");
}

#[test]
fn reruns_use_the_same_snapshot_task_filter_and_cursor() {
    let (world, _) = counting(100, 10);
    let cursor = native(EventFilter::Default.kinds(), 1);
    let mut asked = request(EventCursorArg::Native(cursor.clone()));
    asked.limit = 100;
    events(&world.reads(NONE_MARKERS), &root(), &asked, &|out| {
        out.page.decoded <= 10
    })
    .unwrap();
    assert_eq!(world.calls("snapshot"), 1);
    assert_eq!(world.calls("record"), 1);
    assert_eq!(
        world.calls("event_change_token"),
        1,
        "the token binds the log, not the page size"
    );
    for query in world.history.queries.borrow().iter() {
        assert_eq!(query.cursor.as_ref(), Some(&cursor));
        assert_eq!(query.task, task_id(1));
    }
}

#[test]
fn a_rerun_that_fails_is_reported_not_swallowed() {
    let mut world = world();
    let calls = Cell::new(0);
    let failing = std::rc::Rc::new(calls);
    let inner = failing.clone();
    world.history.pages = Box::new(move |query| {
        inner.set(inner.get() + 1);
        if inner.get() > 1 {
            return Err(EventFailure::Store(StoreFailure::Busy));
        }
        let mut page = pages_of(50);
        page.task = Some(query.task.clone());
        Ok(page)
    });
    let outcome = events(
        &world.reads(NONE_MARKERS),
        &root(),
        &request(EventCursorArg::Absent),
        &|_| false,
    );
    assert_eq!(outcome, Err(ReadError::Store(StoreFailure::Busy)));
}

fn with_prompt(stored: hf_domain::ctl::history::StoredPrompt) -> World {
    let mut world = world();
    world.history = History::default();
    world
        .history
        .prompts
        .insert(task_id(1).as_str().to_string(), stored);
    world
}

#[test]
fn the_prompt_read_shares_the_direct_only_gate() {
    for markers in [FULL_MARKER, EMPTY_MARKER] {
        let world = with_prompt(stored_prompt("p"));
        let outcome = prompt(&world.reads(markers), &root(), task_id(1).as_str(), 100);
        assert_eq!(outcome, Err(ReadError::Refused(RefusalKind::Agent)));
        assert_eq!(world.calls("prompt"), 0);
    }
    let world = with_prompt(stored_prompt("p"));
    prompt(
        &world.reads(NONE_MARKERS),
        &root(),
        task_id(1).as_str(),
        100,
    )
    .unwrap();
    assert_eq!(
        world.order(),
        [
            "classify", "view", "snapshot", "damaged", "related", "lookup", "prompt"
        ]
    );
}

#[test]
fn the_prompt_of_a_found_task_with_no_prompt_row_is_a_vanished_row() {
    let world = world();
    let outcome = prompt(
        &world.reads(NONE_MARKERS),
        &root(),
        task_id(1).as_str(),
        100,
    );
    assert!(matches!(outcome, Err(ReadError::Store(_))), "{outcome:?}");
}

#[test]
fn the_prompt_view_follows_the_stored_form() {
    let mut stored = stored_prompt("hello");
    let world = with_prompt(stored.clone());
    let out = prompt(&world.reads(NONE_MARKERS), &root(), task_id(1).as_str(), 3).unwrap();
    assert_eq!(
        (
            out.view.text.as_deref(),
            out.view.truncated,
            out.view.complete
        ),
        (Some("hel"), true, false)
    );
    stored.inline = None;
    stored.preview = Some("the preview".into());
    stored.bound = true;
    stored.blob = PromptBlob::Absent;
    let world = with_prompt(stored);
    let out = prompt(
        &world.reads(NONE_MARKERS),
        &root(),
        task_id(1).as_str(),
        100,
    )
    .unwrap();
    assert_eq!(
        (out.view.state, out.view.integrity),
        ("preview_only", "not_applicable")
    );
}

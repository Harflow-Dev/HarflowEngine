#![allow(dead_code, unused_imports)]
//! Shared scripted world and helpers of the event page tests.
use super::task_reads_support::{History, NONE_MARKERS, World, page_of, record, root, task_id};
use hf_application::{
    ctl::task_reads::{EventCursorArg, EventsOut, EventsRequest, ReadError, events},
    event_read::EventReadResult,
};
use hf_domain::event_read::{Direction, EventCursor, EventFilter};

pub const STORE: &str = "0123456789abcdef0123456789abcdef";

pub fn world() -> World {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), None));
    world
}

pub fn request(cursor: EventCursorArg) -> EventsRequest {
    EventsRequest {
        task_id: task_id(1).as_str().to_string(),
        limit: 100,
        filter: EventFilter::Default,
        from_oldest: false,
        cursor,
    }
}

pub fn run(
    world: &World,
    markers: hf_domain::ctl::caller::EnvMarkers,
    cursor: EventCursorArg,
) -> Result<EventsOut, ReadError> {
    events(&world.reads(markers), &root(), &request(cursor), &|_| true)
}

pub fn native(kinds: Vec<String>, task: u32) -> EventCursor {
    EventCursor {
        store: STORE.into(),
        generation: STORE.into(),
        task: task_id(task),
        kinds,
        direction: Direction::Older,
        boundary: 10,
        resync: false,
        neighbour_seq: Some(3),
        neighbour_skip: false,
    }
}

pub fn pages_of(count: usize) -> EventReadResult {
    let mut page = page_of(&task_id(1));
    page.decoded = count;
    page
}

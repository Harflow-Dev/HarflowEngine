#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::event_read::{
    EventAuthority, EventAuthorityPort, EventReadError, EventReadPort, EventReadResult, EventReader,
};
use hf_domain::event_read::{EventFilter, EventQuery};
use hf_domain::task_read::TaskId;
use std::{cell::Cell, rc::Rc, time::Instant};

struct Authority {
    value: EventAuthority,
    calls: Rc<Cell<usize>>,
}
impl EventAuthorityPort for Authority {
    fn prepare(&self) -> EventAuthority {
        self.calls.set(self.calls.get() + 1);
        self.value
    }
}
struct Repository {
    calls: Rc<Cell<usize>>,
}
impl EventReadPort for Repository {
    fn execute(&self, _: &EventQuery, _: Instant) -> Result<EventReadResult, EventReadError> {
        self.calls.set(self.calls.get() + 1);
        panic!("repository must not be reached for unauthorized reads")
    }
}
fn query() -> EventQuery {
    EventQuery {
        task: TaskId::new("event-task-1234".into()).unwrap(),
        limit: 10,
        filter: EventFilter::Default,
        from_oldest: false,
        cursor: None,
    }
}

#[test]
fn authority_refuses_preparing_managed_and_undecidable_before_repository_resolution() {
    for (authority_value, expected) in [
        (EventAuthority::Preparing, EventReadError::Preparing),
        (EventAuthority::Managed, EventReadError::Refused),
        (EventAuthority::Blocked, EventReadError::Refused),
    ] {
        let authority_calls = Rc::new(Cell::new(0));
        let repository_calls = Rc::new(Cell::new(0));
        let reader = EventReader::new(
            Authority {
                value: authority_value,
                calls: authority_calls.clone(),
            },
            Repository {
                calls: repository_calls.clone(),
            },
        );
        assert_eq!(reader.read(&query(), Instant::now()), Err(expected));
        assert_eq!(authority_calls.get(), 1);
        assert_eq!(repository_calls.get(), 0);
    }
}

#[test]
fn query_usage_is_validated_before_authority_and_task_resolution() {
    let authority_calls = Rc::new(Cell::new(0));
    let repository_calls = Rc::new(Cell::new(0));
    let reader = EventReader::new(
        Authority {
            value: EventAuthority::Blocked,
            calls: authority_calls.clone(),
        },
        Repository {
            calls: repository_calls.clone(),
        },
    );
    let mut invalid = query();
    invalid.limit = 0;
    assert_eq!(
        reader.read(&invalid, Instant::now()),
        Err(EventReadError::InvalidQuery(
            hf_domain::event_read::EventError::InvalidLimit
        ))
    );
    assert_eq!(authority_calls.get(), 0);
    assert_eq!(repository_calls.get(), 0);
}

#[test]
fn from_oldest_and_cursor_conflict_is_rejected_before_authority() {
    let authority_calls = Rc::new(Cell::new(0));
    let repository_calls = Rc::new(Cell::new(0));
    let reader = EventReader::new(
        Authority {
            value: EventAuthority::Blocked,
            calls: authority_calls.clone(),
        },
        Repository {
            calls: repository_calls.clone(),
        },
    );
    let mut invalid = query();
    invalid.from_oldest = true;
    invalid.cursor = Some(hf_domain::event_read::EventCursor {
        store: "a".repeat(32),
        generation: "b".repeat(32),
        task: invalid.task.clone(),
        kinds: EventFilter::Default.kinds(),
        direction: hf_domain::event_read::Direction::Newer,
        boundary: 1,
        resync: false,
        neighbour_seq: None,
        neighbour_skip: false,
    });
    assert_eq!(
        reader.read(&invalid, Instant::now()),
        Err(EventReadError::InvalidQuery(
            hf_domain::event_read::EventError::InvalidOptions
        ))
    );
    assert_eq!(authority_calls.get(), 0);
    assert_eq!(repository_calls.get(), 0);
}

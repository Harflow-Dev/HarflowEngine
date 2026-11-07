//! The event page read: cursor classification, the page, the log object, the change token
//! and the fit of the page to the document bound.
use super::{
    ReadError, Reads, TaskSnapshotPort, gate::timely, history::direct_read,
    history_ports::EventFailure, resolve::drift,
};
use crate::event_read::EventReadResult;
use hf_domain::{
    ctl::{history::LogObject, state_root::StateRoot},
    event_read::{EventCursor, EventFilter, EventQuery},
    task_read::TaskId,
};

/// Reruns of an oversized page: a binary search over at most 200 limits.
const FIT_RERUNS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventCursorArg {
    Absent,
    /// Not decodable, or of an unknown version or shape.
    Malformed,
    /// A cursor of this engine's generation.
    Native(EventCursor),
    /// A well-formed cursor of another generation: its task and filter digests.
    Foreign {
        task: String,
        filter: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventsRequest {
    pub task_id: String,
    pub limit: usize,
    pub filter: EventFilter,
    pub from_oldest: bool,
    pub cursor: EventCursorArg,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventsOut {
    pub task: TaskId,
    pub resolved_from: Option<String>,
    pub page: EventReadResult,
    pub log: LogObject,
    pub change_token: String,
}

fn read_page(
    snapshot: &dyn TaskSnapshotPort,
    query: &EventQuery,
) -> Result<EventReadResult, ReadError> {
    snapshot.event_page(query).map_err(ReadError::from)
}

/// The query the request means for the resolved task. A foreign cursor is classified here:
/// one that names this task and filter is expired, any other is invalid. Any invalidity a
/// storage read reports after this is a stored row that does not decode, never a cursor.
fn bind(
    snapshot: &dyn TaskSnapshotPort,
    request: &EventsRequest,
    id: &TaskId,
) -> Result<EventQuery, ReadError> {
    let cursor = match &request.cursor {
        EventCursorArg::Absent => None,
        EventCursorArg::Malformed => return Err(ReadError::CursorInvalid),
        EventCursorArg::Native(cursor) => Some(cursor.clone()),
        EventCursorArg::Foreign { task, filter } => {
            let kinds = request.filter.kinds();
            return Err(if snapshot.foreign_cursor_binds(id, &kinds, task, filter) {
                ReadError::CursorExpired
            } else {
                ReadError::CursorInvalid
            });
        }
    };
    let query = EventQuery {
        task: id.clone(),
        limit: request.limit,
        filter: request.filter.clone(),
        from_oldest: request.from_oldest,
        cursor,
    };
    query
        .validate()
        .map_err(|_| ReadError::InvalidParams("invalid event query".to_string()))?;
    query
        .validate_cursor(id)
        .map_err(|_| ReadError::CursorInvalid)?;
    Ok(query)
}

/// One page of a task's event log. `fits` says whether the whole rendered document is within
/// its bound; an oversized page is read again in the same snapshot with a smaller limit, which
/// drops lines from its far side and moves its cursors with them.
pub fn events(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &EventsRequest,
    fits: &dyn Fn(&EventsOut) -> bool,
) -> Result<EventsOut, ReadError> {
    direct_read(
        reads,
        root,
        &request.task_id,
        &mut |snapshot, id, resolved_from| {
            let query = bind(snapshot, request, id)?;
            let page = read_page(snapshot, &query)?;
            // The lifecycle is judged from the record as it is after the log was read.
            let record = snapshot.record(id)?.ok_or_else(drift)?;
            let stream = page.facts.clone().ok_or_else(drift)?;
            let facts = &record.facts;
            let log = LogObject::derive(
                &facts.status,
                facts.exit_code,
                facts.output_drained,
                &stream,
            );
            let change_token = snapshot.event_change_token(&record, &log)?;
            let first = EventsOut {
                task: id.clone(),
                resolved_from,
                page,
                log,
                change_token,
            };
            if fits(&first) {
                return Ok(first);
            }
            shrink(snapshot, reads, &query, first, fits)
        },
    )
}

/// The largest limit below the page's decoded count whose document fits; limit 1 always
/// returns a page, so there is no exhausted case.
fn shrink(
    snapshot: &dyn TaskSnapshotPort,
    reads: &Reads<'_>,
    query: &EventQuery,
    first: EventsOut,
    fits: &dyn Fn(&EventsOut) -> bool,
) -> Result<EventsOut, ReadError> {
    let with = |limit: usize| -> Result<EventsOut, ReadError> {
        timely(reads.deadline)?;
        let page = read_page(
            snapshot,
            &EventQuery {
                limit,
                ..query.clone()
            },
        )?;
        Ok(EventsOut {
            page,
            ..first.clone()
        })
    };
    let (mut low, mut high) = (1usize, first.page.decoded.saturating_sub(1));
    let mut best: Option<EventsOut> = None;
    for _ in 0..FIT_RERUNS {
        if low > high {
            break;
        }
        let middle = low + (high - low) / 2;
        let candidate = with(middle)?;
        if fits(&candidate) {
            best = Some(candidate);
            low = middle + 1;
        } else if middle == 1 {
            break;
        } else {
            high = middle - 1;
        }
    }
    match best {
        Some(found) => Ok(found),
        None => with(1),
    }
}

impl From<EventFailure> for ReadError {
    fn from(failure: EventFailure) -> Self {
        match failure {
            EventFailure::Store(store) => Self::Store(store),
            EventFailure::Expired => Self::CursorExpired,
        }
    }
}

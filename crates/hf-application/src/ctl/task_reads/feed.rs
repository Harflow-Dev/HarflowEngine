//! `interaction-events`: a bounded cycle over the settled tasks, in pages of at most 25
//! sources. This engine stores no workflow or approval facts, so the workflow phase has no
//! sources and the task phase carries every event.
use super::{
    ReadError, Reads,
    feed_ports::FeedPort,
    gate::{Entry, Family, Observer, authorize, check_damage, enter_checked},
};
use hf_domain::ctl::{
    caller::GatePath,
    feed::{
        FeedEvent, FeedSource, FeedState, METADATA_LIMIT, PAYLOAD_LIMIT, Phase, SOURCE_LIMIT,
        candidate,
    },
    state_root::StateRoot,
};

const LIMIT_MAX: i64 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedCursorArg {
    Absent,
    Malformed,
    State(FeedState),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedRequest {
    pub limit: i64,
    pub cursor: FeedCursorArg,
}

/// A source the feed could not read, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedGap {
    pub source: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeedPage {
    pub events: Vec<FeedEvent>,
    /// The state to continue from; `None` when the cycle is complete.
    pub next: Option<FeedState>,
    pub gaps: Vec<FeedGap>,
}

fn error(message: &str) -> ReadError {
    ReadError::Coded {
        code: "interaction_events_error",
        message: message.to_string(),
    }
}

/// The cursor, then the limit; both after the gate.
fn validated(request: &FeedRequest) -> Result<(FeedState, usize), ReadError> {
    let state = match &request.cursor {
        FeedCursorArg::Absent => FeedState::start(),
        FeedCursorArg::Malformed => return Err(error("Invalid interaction-events cursor")),
        FeedCursorArg::State(state) => state.clone(),
    };
    if !(1..=LIMIT_MAX).contains(&request.limit) {
        return Err(error("limit must be between 1 and 100"));
    }
    let limit =
        usize::try_from(request.limit).map_err(|_| error("limit must be between 1 and 100"))?;
    Ok((state, limit))
}

fn empty() -> FeedPage {
    FeedPage {
        events: Vec::new(),
        next: None,
        gaps: Vec::new(),
    }
}

/// The ordering key of an event: its time as a zero-padded fixed-point number, the phase of
/// its state and its id. Compared as text.
pub fn event_key(event: &FeedEvent) -> String {
    let phase = if event.state == "pending" { 0 } else { 1 };
    format!(
        "{:024.6}:{phase}:task:{}:{}",
        event.occurred_at,
        event.task.as_str(),
        event.state
    )
}

/// Returns the page of events for `request`, with `size` the bytes the events of a page take
/// in the document.
pub fn interaction_events(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &FeedRequest,
    size: &dyn Fn(&[FeedEvent]) -> usize,
) -> Result<FeedPage, ReadError> {
    let (marker, view) = match enter_checked(reads, root, GatePath::DirectOnly, &|| Ok(()))? {
        Entry::NoStore => {
            validated(request)?;
            return Ok(empty());
        }
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut page = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            check_damage(snapshot)?;
            let path = GatePath::DirectOnly;
            authorize(
                path,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let (state, limit) = validated(request)?;
            page = Some(walk(snapshot, state, limit, size)?);
            Ok(())
        })?;
    page.ok_or(ReadError::Internal)
}

/// The source the walk reads next: the one a cursor names, even below `after`, or the first
/// after it. `None` when the phase has no more sources; `Some(None)` for a named source that
/// does not exist.
fn next_source(
    feeds: &dyn FeedPort,
    state: &FeedState,
) -> Result<Option<(String, Option<FeedSource>)>, ReadError> {
    if !state.source.is_empty() {
        let found = match state.phase {
            Phase::Workflow => None,
            Phase::Task => feeds.feed_source(&state.source)?,
        };
        return Ok(Some((state.source.clone(), found)));
    }
    if state.phase == Phase::Workflow {
        return Ok(None);
    }
    let first = feeds.feed_sources(&state.after, 1)?.into_iter().next();
    Ok(first.map(|source| (source.id.clone(), Some(source))))
}

fn walk(
    feeds: &dyn FeedPort,
    mut state: FeedState,
    limit: usize,
    size: &dyn Fn(&[FeedEvent]) -> usize,
) -> Result<FeedPage, ReadError> {
    let mut events: Vec<FeedEvent> = Vec::new();
    let mut gaps: Vec<FeedGap> = Vec::new();
    let mut reads = 0usize;
    let mut bytes = 0u64;
    while reads < SOURCE_LIMIT {
        let Some((identifier, found)) = next_source(feeds, &state)? else {
            if state.phase == Phase::Workflow {
                state = FeedState {
                    phase: Phase::Task,
                    ..FeedState::start()
                };
                continue;
            }
            return Ok(FeedPage {
                events,
                next: None,
                gaps,
            });
        };
        reads += 1;
        let mut candidates = Vec::new();
        match found {
            Some(source) => {
                if bytes + source.metadata_bytes > METADATA_LIMIT && reads > 1 {
                    break;
                }
                bytes += source.metadata_bytes;
                candidates.extend(candidate(&source));
            }
            None => gaps.push(FeedGap {
                source: identifier.clone(),
                reason: "source_unavailable",
            }),
        }
        for event in candidates {
            let key = event_key(&event);
            if key <= state.event {
                continue;
            }
            events.push(event);
            if events.len() > limit || size(&events) > PAYLOAD_LIMIT {
                events.pop();
                state.source = identifier;
                return Ok(FeedPage {
                    events,
                    next: Some(state),
                    gaps,
                });
            }
            state.event = key;
        }
        state.after = identifier;
        state.source = String::new();
        state.event = String::new();
    }
    Ok(FeedPage {
        events,
        next: Some(state),
        gaps,
    })
}

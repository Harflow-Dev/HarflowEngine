//! Values of the interaction-events feed: the cursor state, a task source and the event it
//! can yield.
use super::statistics_time::micros_to_seconds;
use crate::task_read::TaskId;

/// Sources read in one page.
pub const SOURCE_LIMIT: usize = 25;
/// Cumulative stored bytes of the sources of one page.
pub const METADATA_LIMIT: u64 = 8 * 1024 * 1024;
/// The bytes the events of a page may take.
pub const PAYLOAD_LIMIT: usize = 128 * 1024;
const TITLE_CHARS: usize = 200;
/// The characters of a state field, and of an `after` or `source` identifier.
pub const FIELD_CHARS: usize = 512;
const IDENTIFIER_CHARS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Workflow runs, which this engine stores no facts for.
    Workflow,
    Task,
}

/// Where a feed walk stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedState {
    pub phase: Phase,
    pub after: String,
    pub source: String,
    pub event: String,
}

impl FeedState {
    pub fn start() -> Self {
        Self {
            phase: Phase::Workflow,
            after: String::new(),
            source: String::new(),
            event: String::new(),
        }
    }
}

/// `[A-Za-z0-9][A-Za-z0-9_.-]{0,99}`
pub fn identifier_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    let first = bytes.first().is_some_and(u8::is_ascii_alphanumeric);
    let rest = bytes
        .iter()
        .skip(1)
        .all(|b| b.is_ascii_alphanumeric() || matches_punctuation(*b));
    first && rest && bytes.len() <= IDENTIFIER_CHARS
}

fn matches_punctuation(byte: u8) -> bool {
    byte == b'_' || byte == b'.' || byte == b'-'
}

/// A task source of the feed: its facts and the stored bytes counted against the budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedSource {
    pub id: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub title: Option<String>,
    pub started_at_us: i64,
    pub finished_at_us: Option<i64>,
    pub metadata_bytes: u64,
}

/// The event of a settled task.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedEvent {
    pub task: TaskId,
    pub state: String,
    pub title: String,
    pub occurred_at: f64,
}

/// A task yields one event only once it settled as `completed` or `failed` with an observed
/// exit code. Cancelled, timed-out and unobserved outcomes yield none.
pub fn candidate(source: &FeedSource) -> Option<FeedEvent> {
    let settled = source.status == "completed" || source.status == "failed";
    if !settled || source.exit_code.is_none() {
        return None;
    }
    let task = TaskId::new(source.id.clone()).ok()?;
    let title = match source.title.as_deref() {
        Some(text) if !text.is_empty() => text,
        _ => source.id.as_str(),
    };
    let stamp = source.finished_at_us.unwrap_or(source.started_at_us);
    Some(FeedEvent {
        task,
        state: source.status.clone(),
        title: title.chars().take(TITLE_CHARS).collect(),
        occurred_at: micros_to_seconds(stamp),
    })
}

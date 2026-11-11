//! Authority-gated task reads: `status`, `status-batch`, `list` and the page read.
//!
//! The order of every path lives here, once: root classification, markers, the process
//! view, then one store snapshot holding the damage check, caller detection, resolution
//! and the read. Nothing a refused caller could learn is read before the gate.
mod artifacts;
mod events;
mod feed;
pub mod feed_ports;
mod gate;
mod history;
pub mod history_ports;
mod list;
mod page;
mod page_parts;
mod page_query;
pub mod ports;
mod recovery;
mod resolve;
mod single;
mod statistics;
mod statistics_page;

pub use artifacts::{
    ArtifactCursorArg, ArtifactEntry, ArtifactPage, ArtifactRequest, NextCursor, artifacts,
};
pub use events::{EventCursorArg, EventsOut, EventsRequest, events};
pub use feed::{FeedCursorArg, FeedGap, FeedPage, FeedRequest, interaction_events};
pub use feed_ports::{FeedPort, ScanBatch, ScanFailure, ScanRow};
pub use history::{PromptOut, prompt};
pub use history_ports::{ArtifactTargetPort, EventFailure, HistoryPort};
pub use list::list;
pub use page::{Counts, CursorArg, ItemSize, PageMode, PageOut, PageRequest, page_with};
pub use ports::{
    ProcessViewPort, RootPort, StoreFailure, TaskSnapshotPort, TaskStorePort, ViewFailure,
};
pub use recovery::{RecoveryOut, recovery_activity};
pub use resolve::missing_resolution;
pub use single::{BatchOut, BatchRow, StatusOut, status, status_batch, status_with};
pub use statistics::{
    EMPTY_GENERATION, SESSION_MARGIN, STATISTICS_BUDGET, StatsCursorArg, StatsRequest, statistics,
    statistics_limited, statistics_within,
};
pub use statistics_page::{StatsMeta, StatsOut, StatsPage, StatsSizing};

use hf_domain::ctl::{
    caller::{EnvMarkers, RefusalKind},
    inspect::StateInspection,
    task_projection::Projection,
    task_rows::TaskRecord,
};
use std::time::Instant;

/// Everything a read needs, supplied by the composition root.
pub struct Reads<'a> {
    pub roots: &'a dyn RootPort,
    pub store: &'a dyn TaskStorePort,
    pub process: &'a dyn ProcessViewPort,
    pub markers: EnvMarkers,
    pub deadline: Instant,
}

/// One task with its live projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskView {
    pub record: TaskRecord,
    pub projection: Projection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnknownReason {
    PrefixTooShort,
    NoPrefixMatch,
    PrefixIndexUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub task_id: String,
    pub title: Option<String>,
    pub status: String,
    pub created_at: String,
}

/// A value that names no single task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Unknown {
        input: String,
        reason: Option<UnknownReason>,
    },
    Ambiguous {
        input: String,
        candidates: Vec<Candidate>,
        truncated: bool,
    },
}

/// Why an artifact cursor is refused: malformed or out of range, or bound to another owner
/// or manifest revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactCursorFailure {
    Invalid,
    Stale,
}

impl ArtifactCursorFailure {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Invalid => "cursor_invalid",
            Self::Stale => "cursor_stale",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    Store(StoreFailure),
    /// The root cannot be read as a store.
    State(StateInspection),
    /// The caller gate refused the read.
    Refused(RefusalKind),
    /// Some task record in the root is damaged.
    Damaged,
    /// A use case ended without an answer or an error; never expected.
    Internal,
    InvalidTaskId(String),
    InvalidParams(String),
    /// An artifact cursor the read refuses; `data.reason` names which kind.
    ArtifactCursor(ArtifactCursorFailure),
    Resolution(Resolution),
    /// The id names a task whose launch has not produced a record. `raw` is the id as typed.
    PendingLaunch {
        raw: String,
        state: String,
        not_started_reason: Option<String>,
    },
    /// An event cursor that is malformed, or belongs to another task, filter or version.
    CursorInvalid,
    /// A well-formed event cursor that no longer applies.
    CursorExpired,
    /// A failure with its own wire code and message: a refusal of the feed and statistics
    /// requests, or an unknown task without a resolution diagnostic.
    Coded {
        code: &'static str,
        message: String,
    },
}

impl From<StoreFailure> for ReadError {
    fn from(failure: StoreFailure) -> Self {
        Self::Store(failure)
    }
}

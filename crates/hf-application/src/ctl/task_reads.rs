//! Authority-gated task reads: `status`, `status-batch`, `list` and the page read.
//!
//! The order of every path lives here, once: root classification, markers, the process
//! view, then one store snapshot holding the damage check, caller detection, resolution
//! and the read. Nothing a refused caller could learn is read before the gate.
mod artifacts;
mod events;
mod gate;
mod history;
pub mod history_ports;
mod list;
mod page;
mod page_parts;
mod page_query;
pub mod ports;
mod resolve;
mod single;

pub use artifacts::{
    ArtifactCursorArg, ArtifactEntry, ArtifactPage, ArtifactRequest, NextCursor, artifacts,
};
pub use events::{EventCursorArg, EventsOut, EventsRequest, events};
pub use history::{PromptOut, prompt};
pub use history_ports::{ArtifactTargetPort, EventFailure, HistoryPort};
pub use list::list;
pub use page::{Counts, CursorArg, ItemSize, PageMode, PageOut, PageRequest, page_with};
pub use ports::{
    ProcessViewPort, RootPort, StoreFailure, TaskSnapshotPort, TaskStorePort, ViewFailure,
};
pub use resolve::missing_resolution;
pub use single::{BatchOut, BatchRow, StatusOut, status, status_batch, status_with};

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
    Resolution(Resolution),
    /// The id names a task whose launch has not produced a record.
    PendingLaunch(String),
    /// An event cursor that is malformed, or belongs to another task, filter or version.
    CursorInvalid,
    /// A well-formed event cursor that no longer applies.
    CursorExpired,
}

impl From<StoreFailure> for ReadError {
    fn from(failure: StoreFailure) -> Self {
        Self::Store(failure)
    }
}

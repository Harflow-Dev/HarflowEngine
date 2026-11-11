//! Ports of the task reads: the root classifier, the store snapshot and the process view.
use super::{ReadError, feed_ports::FeedPort, history_ports::HistoryPort};
use crate::ctl::state_inspect::RootClass;
use hf_domain::{
    ctl::{
        caller::{PidObservation, ProcessView},
        continuation_stop::StopRow,
        inspect::InspectReason,
        state_root::StateRoot,
        task_rows::{BatchQuery, CallerRow, TaskRecord, TaskSummary, UnobservedRow},
    },
    task_read::{Lookup, TaskId},
};
use std::time::Instant;

pub trait RootPort {
    fn classify(&self, root: &StateRoot) -> RootClass;
}

/// A store failure the transport maps to a wire code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreFailure {
    /// `state_busy`: a lease, the database or the deadline was not available in time.
    Busy,
    /// `state_incompatible` with this reason and message.
    Incompatible {
        reason: InspectReason,
        message: String,
    },
}

/// The named queries of one read transaction. Every answer comes from the same snapshot.
pub trait TaskSnapshotPort: HistoryPort + FeedPort {
    /// Whether any task record is damaged, root-wide.
    fn damaged(&self) -> Result<bool, StoreFailure>;
    /// Unfinished tasks whose recorded session is `session` or whose process is one of
    /// `ancestors`. With `Some(limit)` at most `limit + 1` rows are returned.
    fn related_callers(
        &self,
        session: i64,
        ancestors: &[u32],
        limit: Option<usize>,
    ) -> Result<Vec<CallerRow>, StoreFailure>;
    fn lookup(&self, id: &TaskId) -> Result<Lookup, StoreFailure>;
    fn summaries(&self, ids: &[TaskId]) -> Result<Vec<TaskSummary>, StoreFailure>;
    fn record(&self, id: &TaskId) -> Result<Option<TaskRecord>, StoreFailure>;
    /// The record with the stop row of its chain root, from the same snapshot.
    fn record_with_stop(&self, id: &TaskId) -> Result<Option<(TaskRecord, StopRow)>, StoreFailure>;
    /// Newest first; stops early at the byte limit and never returns an empty batch while
    /// rows remain.
    fn batch(&self, query: &BatchQuery) -> Result<Vec<TaskRecord>, StoreFailure>;
    fn unobserved_count(&self) -> Result<u64, StoreFailure>;
    fn unobserved_rows(
        &self,
        after: Option<(i64, String)>,
        max_rows: usize,
    ) -> Result<Vec<UnobservedRow>, StoreFailure>;
    /// The digest a page cursor is bound to.
    fn cursor_scope(
        &self,
        active_only: bool,
        session: Option<&str>,
        predecessor: Option<&TaskId>,
    ) -> Result<String, StoreFailure>;
}

pub trait TaskStorePort {
    /// Opens the store read-only and runs `action` inside one snapshot.
    fn with_snapshot(
        &self,
        root: &StateRoot,
        deadline: Instant,
        action: &mut dyn FnMut(&dyn TaskSnapshotPort) -> Result<(), ReadError>,
    ) -> Result<(), ReadError>;
}

/// The process table could not be read, or this process is not in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFailure;

pub trait ProcessViewPort {
    fn view(&self, deadline: Instant) -> Result<ProcessView, ViewFailure>;
    fn observe(&self, pid: u32) -> PidObservation;
}

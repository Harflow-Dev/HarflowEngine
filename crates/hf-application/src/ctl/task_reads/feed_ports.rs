//! Ports of the recovery-activity, interaction-events and statistics reads, all answered from
//! the one snapshot.
use super::StoreFailure;
use hf_domain::{
    ctl::{
        feed::FeedSource,
        statistics::{GenerationEntry, StatFacts},
    },
    event_read::CaptureState,
    task_read::TaskId,
};
use std::time::Instant;

/// One row of the statistics scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanRow {
    Facts(Box<StatFacts>),
    /// A row too large to read; only its id is known.
    Oversized(String),
}

impl ScanRow {
    pub fn id(&self) -> &str {
        match self {
            Self::Facts(facts) => facts.id.as_str(),
            Self::Oversized(id) => id,
        }
    }
}

/// One batch of the statistics scan. `expired` says the deadline passed before the batch was
/// complete: the rows are those read in time, and the scan has not reached its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanBatch {
    pub rows: Vec<ScanRow>,
    pub expired: bool,
}

/// A scan that could not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanFailure {
    /// The read deadline ran out inside the store.
    Deadline,
    /// A row or batch beyond the query budget.
    Limit,
    Store(StoreFailure),
}

impl From<StoreFailure> for ScanFailure {
    fn from(failure: StoreFailure) -> Self {
        Self::Store(failure)
    }
}

pub trait FeedPort {
    /// The capture the task's stream row stores; `None` without a stream row.
    fn stream_capture(&self, id: &TaskId) -> Result<Option<CaptureState>, StoreFailure>;
    /// Up to `limit` task sources with an id greater than `after`, in byte order of the id.
    fn feed_sources(&self, after: &str, limit: usize) -> Result<Vec<FeedSource>, StoreFailure>;
    /// One task source by id; `None` when no valid task has it.
    fn feed_source(&self, id: &str) -> Result<Option<FeedSource>, StoreFailure>;
    /// How many task rows of any validity there are, counted up to `limit`.
    fn history_rows(&self, limit: usize) -> Result<usize, StoreFailure>;
    /// How many valid task rows there are, counted up to `limit`.
    fn valid_rows(&self, limit: usize) -> Result<usize, StoreFailure>;
    /// The next batch of the statistics scan: valid tasks with an id greater than `after`, in
    /// byte order of the id, bounded by rows and by bytes, and by `deadline`, whose passing ends the batch
    /// early with `expired` set. An empty batch that has not expired ends the scan.
    fn statistics_batch(
        &self,
        after: Option<&str>,
        deadline: Instant,
    ) -> Result<ScanBatch, ScanFailure>;
    /// The generation digest of the relevant tasks of a non-empty projection.
    fn statistics_generation(&self, entries: &[GenerationEntry]) -> Result<String, StoreFailure>;
}

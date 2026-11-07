//! Ports of the history reads: event pages, prompts, artifact manifests and the target
//! files artifacts name.
use super::StoreFailure;
use crate::event_read::EventReadResult;
use hf_domain::{
    ctl::{
        history::{ArtifactItem, Availability, LogObject, ManifestHead, StoredPrompt},
        task_rows::TaskRecord,
    },
    event_read::EventQuery,
    task_read::TaskId,
};

/// An event page that could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventFailure {
    Store(StoreFailure),
    /// The cursor names a store, stream or log extent that is no longer served.
    Expired,
}

impl From<StoreFailure> for EventFailure {
    fn from(failure: StoreFailure) -> Self {
        Self::Store(failure)
    }
}

/// The history queries of one read transaction, all from the same snapshot.
pub trait HistoryPort {
    fn event_page(&self, query: &EventQuery) -> Result<EventReadResult, EventFailure>;
    /// The digest of the record, the store and stream extents and the log object.
    fn event_change_token(
        &self,
        record: &TaskRecord,
        log: &LogObject,
    ) -> Result<String, StoreFailure>;
    /// Whether a cursor of another generation names this task and these kinds.
    fn foreign_cursor_binds(
        &self,
        task: &TaskId,
        kinds: &[String],
        task_digest: &str,
        filter_digest: &str,
    ) -> bool;
    /// `None` when the task has no prompt row.
    fn prompt(&self, id: &TaskId, max_bytes: usize) -> Result<Option<StoredPrompt>, StoreFailure>;
    fn artifact_manifest(&self, id: &TaskId) -> Result<Option<ManifestHead>, StoreFailure>;
    fn artifact_items(
        &self,
        id: &TaskId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ArtifactItem>, StoreFailure>;
    /// `None` when the task is missing or damaged; `Some(None)` when it continues nothing.
    fn continuation_link(&self, id: &TaskId) -> Result<Option<Option<TaskId>>, StoreFailure>;
    /// Stored event bodies from the complete log's last megabyte, at most 1 MiB of kept
    /// bytes, oldest first.
    fn recent_event_bodies(&self, id: &TaskId) -> Result<Vec<Vec<u8>>, StoreFailure>;
    /// Whether the task is a workflow builder, which artifact reads refuse.
    fn workflow_builder(&self, id: &TaskId) -> Result<bool, StoreFailure>;
}

/// Observes what a reference's target file is now. It reads no content and writes nothing.
pub trait ArtifactTargetPort {
    fn availability(&self, repo_path: &str, item: &ArtifactItem) -> Availability;
    /// A failure met while observing that ends the whole read instead of one artifact, as
    /// the reference lets a missing account name escape. The text is the error message.
    fn command_failure(&self) -> Option<String>;
}

//! The history queries of the read-only snapshot: event pages, prompts and artifacts.
use super::store_reads::{Snapshot, failure, read};
use hf_application::{
    ctl::task_reads::{EventFailure, HistoryPort, StoreFailure},
    event_read::EventReadResult,
};
use hf_domain::{
    ctl::{
        history::{ArtifactItem, LogObject, ManifestHead, StoredPrompt},
        task_rows::TaskRecord,
    },
    event_read::EventQuery,
    task_read::TaskId,
};
use hf_storage::Error;

impl HistoryPort for Snapshot<'_, '_> {
    fn event_page(&self, query: &EventQuery) -> Result<EventReadResult, EventFailure> {
        self.0.event_page(query).map_err(|error| match error {
            Error::ExpiredEventCursor => EventFailure::Expired,
            other => EventFailure::Store(failure(&other)),
        })
    }
    fn event_change_token(
        &self,
        record: &TaskRecord,
        log: &LogObject,
    ) -> Result<String, StoreFailure> {
        read(self.0.event_change_token(record, log))
    }
    fn foreign_cursor_binds(
        &self,
        task: &TaskId,
        kinds: &[String],
        task_digest: &str,
        filter_digest: &str,
    ) -> bool {
        self.0
            .foreign_cursor_binds(task, kinds, task_digest, filter_digest)
    }
    fn prompt(&self, id: &TaskId, max_bytes: usize) -> Result<Option<StoredPrompt>, StoreFailure> {
        read(self.0.prompt(id, max_bytes))
    }
    fn artifact_manifest(&self, id: &TaskId) -> Result<Option<ManifestHead>, StoreFailure> {
        read(self.0.artifact_manifest(id))
    }
    fn artifact_items(
        &self,
        id: &TaskId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ArtifactItem>, StoreFailure> {
        read(self.0.artifact_items(id, offset, limit))
    }
    fn continuation_link(&self, id: &TaskId) -> Result<Option<Option<TaskId>>, StoreFailure> {
        read(self.0.continuation_link(id))
    }
    fn recent_event_bodies(&self, id: &TaskId) -> Result<Vec<Vec<u8>>, StoreFailure> {
        read(self.0.recent_event_bodies(id))
    }
    fn workflow_builder(&self, id: &TaskId) -> Result<bool, StoreFailure> {
        read(self.0.workflow_builder(id))
    }
}

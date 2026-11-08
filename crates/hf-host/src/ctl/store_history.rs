//! The history queries of the read-only snapshot: event pages, prompts and artifacts.
use super::store_reads::{Snapshot, failure, read};
use hf_application::{
    ctl::task_reads::{EventFailure, FeedPort, HistoryPort, ScanBatch, ScanFailure, StoreFailure},
    event_read::EventReadResult,
};
use hf_domain::{
    ctl::{
        feed::FeedSource,
        history::{ArtifactItem, LogObject, ManifestHead, StoredPrompt},
        statistics::GenerationEntry,
        task_rows::TaskRecord,
    },
    event_read::{CaptureState, EventQuery},
    task_read::TaskId,
};
use hf_storage::Error;
use std::time::Instant;

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

impl FeedPort for Snapshot<'_, '_> {
    fn stream_capture(&self, id: &TaskId) -> Result<Option<CaptureState>, StoreFailure> {
        read(self.0.stream_capture(id))
    }
    fn feed_sources(&self, after: &str, limit: usize) -> Result<Vec<FeedSource>, StoreFailure> {
        read(self.0.feed_sources(after, limit))
    }
    fn feed_source(&self, id: &str) -> Result<Option<FeedSource>, StoreFailure> {
        read(self.0.feed_source(id))
    }
    fn history_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        read(self.0.history_rows(limit))
    }
    fn valid_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        read(self.0.valid_rows(limit))
    }
    fn statistics_batch(
        &self,
        after: Option<&str>,
        deadline: Instant,
    ) -> Result<ScanBatch, ScanFailure> {
        self.0
            .statistics_batch(after, deadline)
            .map_err(|error| match error {
                Error::DeadlineExceeded => ScanFailure::Deadline,
                Error::LimitExceeded => ScanFailure::Limit,
                other => ScanFailure::Store(failure(&other)),
            })
    }
    fn statistics_generation(&self, entries: &[GenerationEntry]) -> Result<String, StoreFailure> {
        read(self.0.statistics_generation(entries))
    }
}

//! Scripted history queries behind the `Fake`: pages as a function of the query, one prompt,
//! one manifest with its items, recent bodies and the availability of targets.
use super::{Fake, ID};
use hf_application::{
    ctl::task_reads::{ArtifactTargetPort, EventFailure, HistoryPort, StoreFailure},
    event_read::{EventReadResult, EventRecord, RecordClass},
};
use hf_domain::{
    ctl::{
        history::{ArtifactItem, Availability, LogObject, ManifestHead, StoredPrompt},
        task_rows::TaskRecord,
    },
    event_read::{CaptureState, EventQuery, LogFacts},
    task_read::{Lookup, TaskId},
};
use std::{cell::RefCell, collections::HashMap};

pub type Pages = Box<dyn Fn(&EventQuery) -> Result<EventReadResult, EventFailure>>;

pub struct History {
    pub pages: Pages,
    pub token: String,
    pub binds: bool,
    pub prompt: Option<StoredPrompt>,
    pub manifest: Option<ManifestHead>,
    pub items: Vec<ArtifactItem>,
    pub recent: Vec<Vec<u8>>,
    pub fail: Option<StoreFailure>,
    /// What `workflow_builder` answers for every task.
    pub builder: bool,
    pub queries: RefCell<Vec<EventQuery>>,
}

#[derive(Default)]
pub struct Targets {
    pub states: HashMap<String, Availability>,
    /// A failure that ends the whole command, once any target has been observed.
    pub failure: Option<String>,
}

impl ArtifactTargetPort for Targets {
    fn availability(&self, _: &str, item: &ArtifactItem) -> Availability {
        self.states
            .get(&item.artifact_id)
            .copied()
            .unwrap_or(Availability::Available)
    }
    fn command_failure(&self) -> Option<String> {
        self.failure.clone()
    }
}

pub fn page_of(task: &TaskId) -> EventReadResult {
    EventReadResult {
        task: Some(task.clone()),
        lookup: Lookup::Found(task.clone()),
        events: Vec::new(),
        older_cursor: None,
        newer_cursor: None,
        has_older: false,
        has_newer: false,
        span: None,
        malformed: 0,
        oversized: 0,
        gaps: Vec::new(),
        facts: Some(LogFacts {
            task_status: None,
            available: true,
            capture: CaptureState::Unknown,
            output_drained: None,
            eof_finished_seq: None,
            within_grace: false,
        }),
        decoded: 0,
        scanned_bytes: 0,
    }
}

/// A valid record whose stored body is `body`.
pub fn event_row(start: u64, seq: u64, kind: &str, body: &str) -> EventRecord {
    EventRecord {
        start,
        end: start + body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some(kind.to_string()),
        body: body.as_bytes().to_vec(),
    }
}

impl Default for History {
    fn default() -> Self {
        Self {
            pages: Box::new(|query| Ok(page_of(&query.task))),
            token: "0".repeat(64),
            binds: false,
            prompt: None,
            manifest: None,
            items: Vec::new(),
            recent: Vec::new(),
            fail: None,
            builder: false,
            queries: RefCell::new(Vec::new()),
        }
    }
}

impl HistoryPort for Fake {
    fn event_page(&self, query: &EventQuery) -> Result<EventReadResult, EventFailure> {
        self.history.queries.borrow_mut().push(query.clone());
        if let Some(failure) = &self.history.fail {
            return Err(EventFailure::Store(failure.clone()));
        }
        (self.history.pages)(query)
    }
    fn event_change_token(&self, _: &TaskRecord, _: &LogObject) -> Result<String, StoreFailure> {
        Ok(self.history.token.clone())
    }
    fn foreign_cursor_binds(&self, _: &TaskId, _: &[String], _: &str, _: &str) -> bool {
        self.history.binds
    }
    fn prompt(&self, _: &TaskId, _: usize) -> Result<Option<StoredPrompt>, StoreFailure> {
        Ok(self.history.prompt.clone())
    }
    fn artifact_manifest(&self, _: &TaskId) -> Result<Option<ManifestHead>, StoreFailure> {
        Ok(self.history.manifest.clone())
    }
    fn artifact_items(
        &self,
        _: &TaskId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ArtifactItem>, StoreFailure> {
        Ok(self
            .history
            .items
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }
    fn continuation_link(&self, id: &TaskId) -> Result<Option<Option<TaskId>>, StoreFailure> {
        let _ = (id, ID);
        Ok(Some(None))
    }
    fn workflow_builder(&self, _: &TaskId) -> Result<bool, StoreFailure> {
        Ok(self.history.builder)
    }
    fn recent_event_bodies(&self, _: &TaskId) -> Result<Vec<Vec<u8>>, StoreFailure> {
        Ok(self.history.recent.clone())
    }
}

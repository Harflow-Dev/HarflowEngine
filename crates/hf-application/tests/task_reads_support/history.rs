//! Scripted history queries of the world: event pages from a function of the query, prompts,
//! manifests and items by task, and the order in which they are asked.
use super::World;
use hf_application::{
    ctl::task_reads::{EventFailure, HistoryPort, StoreFailure},
    event_read::EventReadResult,
};
use hf_domain::{
    ctl::{
        history::{ArtifactItem, LogObject, ManifestHead, Observation, PromptBlob, StoredPrompt},
        task_rows::TaskRecord,
    },
    event_read::{CaptureState, EventQuery, LogFacts},
    task_read::{Lookup, TaskId},
};
use std::{cell::RefCell, collections::HashMap};

/// A foreign-cursor question: the task, the kinds and the task and filter digests.
pub type Asked = (String, Vec<String>, String, String);
pub type Pages = Box<dyn Fn(&EventQuery) -> Result<EventReadResult, EventFailure>>;

pub struct History {
    /// Whether the manifest and link queries appear in the call log; the status scenarios of
    /// the older tests do not expect them there.
    pub noisy: bool,
    pub pages: Pages,
    pub token: Result<String, StoreFailure>,
    /// What `foreign_cursor_binds` answers.
    pub binds: bool,
    pub prompts: HashMap<String, StoredPrompt>,
    pub manifests: HashMap<String, ManifestHead>,
    pub items: HashMap<String, Vec<ArtifactItem>>,
    pub recent: Vec<Vec<u8>>,
    /// A failure the recent-bodies query reports instead of the bodies.
    pub recent_error: Option<StoreFailure>,
    /// Tasks whose records say they are workflow builders.
    pub builders: Vec<String>,
    /// Every event query, in order.
    pub queries: RefCell<Vec<EventQuery>>,
    /// Every foreign-cursor question: the task, the kinds and the two digests.
    pub asked: RefCell<Vec<Asked>>,
    /// Every item read: offset and limit.
    pub reads: RefCell<Vec<(usize, usize)>>,
}

pub fn log_facts(capture: CaptureState) -> LogFacts {
    LogFacts {
        task_status: None,
        available: true,
        capture,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
    }
}

/// An empty page of a found task with log facts.
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
        facts: Some(log_facts(CaptureState::Unknown)),
        decoded: 0,
        scanned_bytes: 0,
    }
}

pub fn stored_prompt(inline: &str) -> StoredPrompt {
    StoredPrompt {
        title: None,
        inline: Some(inline.to_string()),
        preview: None,
        bound: false,
        blob: PromptBlob::Absent,
    }
}

pub fn items(count: usize) -> Vec<ArtifactItem> {
    (0..count)
        .map(|n| ArtifactItem {
            artifact_id: format!("x{n}"),
            path: format!("/repo/out/f{n}.txt"),
            kind: "report".into(),
            note: String::new(),
            registered_at: "2026-01-01T00:00:00+00:00".into(),
            method: "verified_process_lineage".into(),
            plan: None,
            registered: Observation {
                device: 1,
                inode: n as u64 + 2,
                size: 3,
                mtime_ns: 4,
            },
        })
        .collect()
}

impl Default for History {
    fn default() -> Self {
        Self {
            noisy: false,
            pages: Box::new(|query| Ok(page_of(&query.task))),
            token: Ok("0".repeat(64)),
            binds: false,
            prompts: HashMap::new(),
            manifests: HashMap::new(),
            items: HashMap::new(),
            recent: Vec::new(),
            recent_error: None,
            builders: Vec::new(),
            queries: RefCell::new(Vec::new()),
            asked: RefCell::new(Vec::new()),
            reads: RefCell::new(Vec::new()),
        }
    }
}

impl HistoryPort for World {
    fn event_page(&self, query: &EventQuery) -> Result<EventReadResult, EventFailure> {
        self.note("event_page".into());
        self.history.queries.borrow_mut().push(query.clone());
        (self.history.pages)(query)
    }
    fn event_change_token(&self, _: &TaskRecord, _: &LogObject) -> Result<String, StoreFailure> {
        self.note("event_change_token".into());
        self.history.token.clone()
    }
    fn foreign_cursor_binds(
        &self,
        task: &TaskId,
        kinds: &[String],
        task_digest: &str,
        filter_digest: &str,
    ) -> bool {
        self.note("foreign_binds".into());
        self.history.asked.borrow_mut().push((
            task.as_str().to_string(),
            kinds.to_vec(),
            task_digest.to_string(),
            filter_digest.to_string(),
        ));
        self.history.binds
    }
    fn prompt(&self, id: &TaskId, _: usize) -> Result<Option<StoredPrompt>, StoreFailure> {
        self.note(format!("prompt:{}", id.as_str()));
        Ok(self.history.prompts.get(id.as_str()).cloned())
    }
    fn artifact_manifest(&self, id: &TaskId) -> Result<Option<ManifestHead>, StoreFailure> {
        if self.history.noisy {
            self.note(format!("manifest:{}", id.as_str()));
        }
        Ok(self.history.manifests.get(id.as_str()).cloned())
    }
    fn artifact_items(
        &self,
        id: &TaskId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ArtifactItem>, StoreFailure> {
        self.note(format!("items:{}", id.as_str()));
        self.history.reads.borrow_mut().push((offset, limit));
        let all = self
            .history
            .items
            .get(id.as_str())
            .cloned()
            .unwrap_or_default();
        Ok(all.into_iter().skip(offset).take(limit).collect())
    }
    fn continuation_link(&self, id: &TaskId) -> Result<Option<Option<TaskId>>, StoreFailure> {
        if self.history.noisy {
            self.note(format!("link:{}", id.as_str()));
        }
        Ok(self
            .tasks
            .iter()
            .find(|task| &task.facts.id == id)
            .map(|task| task.facts.continuation_of.clone()))
    }
    fn workflow_builder(&self, id: &TaskId) -> Result<bool, StoreFailure> {
        Ok(self.history.builders.iter().any(|b| b == id.as_str()))
    }
    fn recent_event_bodies(&self, id: &TaskId) -> Result<Vec<Vec<u8>>, StoreFailure> {
        self.note(format!("recent:{}", id.as_str()));
        self.history
            .recent_error
            .clone()
            .map_or_else(|| Ok(self.history.recent.clone()), Err)
    }
}

//! Scripted feed, recovery and statistics queries behind the `Fake`.
use super::Fake;
use hf_application::ctl::task_reads::{FeedPort, ScanBatch, ScanFailure, ScanRow, StoreFailure};
use hf_domain::{
    ctl::{
        feed::FeedSource,
        statistics::{GenerationEntry, StatFacts},
    },
    event_read::CaptureState,
    task_read::TaskId,
};
use std::{collections::HashMap, time::Instant};

pub struct Feeds {
    pub sources: Vec<FeedSource>,
    pub captures: HashMap<String, CaptureState>,
    pub rows: Vec<ScanRow>,
    pub generation: String,
    pub scan_failure: Option<ScanFailure>,
    pub history_rows: Option<usize>,
    /// How many valid task rows the root holds; the scan rows when unset.
    pub valid_rows: Option<usize>,
}

impl Default for Feeds {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            captures: HashMap::new(),
            rows: Vec::new(),
            generation: "ab".repeat(32),
            scan_failure: None,
            history_rows: None,
            valid_rows: None,
        }
    }
}

pub fn facts(id: &str, start_us: i64, end_us: i64) -> StatFacts {
    StatFacts {
        id: TaskId::new(id.to_string()).unwrap(),
        status: "completed".into(),
        started_at: "2026-01-10T10:00:00+00:00".into(),
        started_at_us: start_us,
        finished_at: Some("2026-01-10T10:05:00+00:00".into()),
        finished_at_us: Some(end_us),
        exit_code: Some(0),
        title: Some("First task".into()),
        backend: "claude".into(),
        model: Some("model-a".into()),
        repo_path: "/workspace/repo".into(),
        continuation_of: None,
        chain_root: None,
    }
}

impl FeedPort for Fake {
    fn stream_capture(&self, id: &TaskId) -> Result<Option<CaptureState>, StoreFailure> {
        Ok(self.feed.captures.get(id.as_str()).copied())
    }
    fn feed_sources(&self, after: &str, limit: usize) -> Result<Vec<FeedSource>, StoreFailure> {
        Ok(self
            .feed
            .sources
            .iter()
            .filter(|s| s.id.as_str() > after)
            .take(limit)
            .cloned()
            .collect())
    }
    fn feed_source(&self, id: &str) -> Result<Option<FeedSource>, StoreFailure> {
        Ok(self.feed.sources.iter().find(|s| s.id == id).cloned())
    }
    fn history_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        Ok(self
            .feed
            .history_rows
            .unwrap_or(self.feed.rows.len())
            .min(limit))
    }
    fn valid_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        Ok(self
            .feed
            .valid_rows
            .unwrap_or(self.feed.rows.len())
            .min(limit))
    }
    fn statistics_batch(&self, after: Option<&str>, _: Instant) -> Result<ScanBatch, ScanFailure> {
        if let Some(failure) = &self.feed.scan_failure {
            return Err(failure.clone());
        }
        let rows = self
            .feed
            .rows
            .iter()
            .filter(|r| after.is_none_or(|a| r.id() > a))
            .take(200)
            .cloned()
            .collect();
        Ok(ScanBatch {
            rows,
            expired: false,
        })
    }
    fn statistics_generation(&self, _: &[GenerationEntry]) -> Result<String, StoreFailure> {
        Ok(self.feed.generation.clone())
    }
}

//! Scripted feed, recovery and statistics queries of the world: sources and scan rows in id
//! order, stored captures, scripted failures, and a record of what was asked.
use super::World;
use hf_application::ctl::task_reads::{FeedPort, ScanBatch, ScanFailure, ScanRow, StoreFailure};
use hf_domain::{
    ctl::{
        feed::FeedSource,
        statistics::{GenerationEntry, StatFacts},
    },
    event_read::CaptureState,
    task_read::TaskId,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::{Duration, Instant},
};

pub struct FeedWorld {
    pub sources: Vec<FeedSource>,
    pub captures: HashMap<String, CaptureState>,
    pub rows: Vec<ScanRow>,
    pub batch_rows: usize,
    /// How many task rows of any validity the root holds; the scan rows when unset.
    pub history_rows: Option<usize>,
    /// How many valid task rows the root holds; the scan rows when unset.
    pub valid_rows: Option<usize>,
    /// The batch with this index (from 0) keeps only this many rows and reports expiry.
    pub expire: Option<(usize, usize)>,
    /// The failure of the batch with this index (from 0), instead of its rows.
    pub scan_failure: Option<(usize, ScanFailure)>,
    pub generation: Result<String, StoreFailure>,
    /// Slept before each batch is answered.
    pub delay: Duration,
    pub batches: Cell<usize>,
    pub entries: RefCell<Vec<Vec<GenerationEntry>>>,
    /// Every `feed_sources` question: after and limit.
    pub asked: RefCell<Vec<(String, usize)>>,
}

impl Default for FeedWorld {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            captures: HashMap::new(),
            rows: Vec::new(),
            batch_rows: 200,
            history_rows: None,
            valid_rows: None,
            expire: None,
            scan_failure: None,
            generation: Ok("g".repeat(64)),
            delay: Duration::ZERO,
            batches: Cell::new(0),
            entries: RefCell::new(Vec::new()),
            asked: RefCell::new(Vec::new()),
        }
    }
}

pub fn source(id: &str, status: &str, exit: Option<i64>) -> FeedSource {
    FeedSource {
        id: id.to_string(),
        status: status.to_string(),
        exit_code: exit,
        title: None,
        started_at_us: 1_000_000,
        finished_at_us: Some(2_000_000),
        metadata_bytes: 100,
    }
}

pub fn stat_facts(id: &str, status: &str, start_us: i64, end_us: Option<i64>) -> StatFacts {
    StatFacts {
        id: TaskId::new(id.to_string()).unwrap(),
        status: status.to_string(),
        started_at: "2026-01-01T00:00:00+00:00".into(),
        started_at_us: start_us,
        finished_at: end_us.map(|_| "2026-01-01T00:00:01+00:00".to_string()),
        finished_at_us: end_us,
        exit_code: Some(0),
        title: None,
        backend: "claude".into(),
        model: None,
        repo_path: "/repo".into(),
        continuation_of: None,
        chain_root: None,
    }
}

pub fn row(facts: StatFacts) -> ScanRow {
    ScanRow::Facts(Box::new(facts))
}

impl FeedPort for World {
    fn stream_capture(&self, id: &TaskId) -> Result<Option<CaptureState>, StoreFailure> {
        self.note(format!("stream_capture:{}", id.as_str()));
        Ok(self.feed.captures.get(id.as_str()).copied())
    }
    fn feed_sources(&self, after: &str, limit: usize) -> Result<Vec<FeedSource>, StoreFailure> {
        self.note("feed_sources".into());
        self.feed
            .asked
            .borrow_mut()
            .push((after.to_string(), limit));
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
        self.note(format!("feed_source:{id}"));
        Ok(self.feed.sources.iter().find(|s| s.id == id).cloned())
    }
    fn history_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        self.note("history_rows".into());
        Ok(self
            .feed
            .history_rows
            .unwrap_or(self.feed.rows.len())
            .min(limit))
    }
    fn valid_rows(&self, limit: usize) -> Result<usize, StoreFailure> {
        self.note("valid_rows".into());
        Ok(self
            .feed
            .valid_rows
            .unwrap_or(self.feed.rows.len())
            .min(limit))
    }
    fn statistics_batch(
        &self,
        after: Option<&str>,
        _deadline: Instant,
    ) -> Result<ScanBatch, ScanFailure> {
        let index = self.feed.batches.get();
        self.feed.batches.set(index + 1);
        self.note("statistics_batch".into());
        std::thread::sleep(self.feed.delay);
        if let Some((at, failure)) = &self.feed.scan_failure
            && *at == index
        {
            return Err(failure.clone());
        }
        let mut rows: Vec<ScanRow> = self
            .feed
            .rows
            .iter()
            .filter(|r| after.is_none_or(|a| r.id() > a))
            .take(self.feed.batch_rows)
            .cloned()
            .collect();
        // The deadline passes inside this batch: only the first rows were read in time.
        let expired = match self.feed.expire {
            Some((at, kept)) if at == index => {
                rows.truncate(kept);
                true
            }
            _ => false,
        };
        Ok(ScanBatch { rows, expired })
    }
    fn statistics_generation(&self, entries: &[GenerationEntry]) -> Result<String, StoreFailure> {
        self.note("statistics_generation".into());
        self.feed.entries.borrow_mut().push(entries.to_vec());
        self.feed.generation.clone()
    }
}

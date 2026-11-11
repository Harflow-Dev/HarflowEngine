#![allow(dead_code, unused_imports)]
//! A scripted world behind the task-read ports: a root class, a process table, records and
//! a log of every port call, so the order and the number of calls can be asserted.
mod feed;
pub mod gate;
mod history;
mod snapshot;
pub use feed::{FeedWorld, row, source, stat_facts};
use hf_application::ctl::task_reads::ReadError;
use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        ProcessViewPort, Reads, RootPort, StoreFailure, TaskSnapshotPort, TaskStorePort,
        ViewFailure,
    },
};
use hf_domain::{
    ctl::{
        caller::{EnvMarkers, PidObservation, ProcessView},
        state_root::StateRoot,
        task_rows::{
            BatchQuery, CallerRow, ChainRoot, ProcessRecord, TaskFacts, TaskRecord, TaskSummary,
            UnobservedRow,
        },
    },
    process::ProcessIdentity,
    task_read::{Lookup, PendingLaunch, TaskId},
};
pub use history::{History, items, log_facts, page_of, stored_prompt};
use std::{cell::RefCell, collections::HashMap, time::Instant};

pub const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";
pub const NONE_MARKERS: EnvMarkers = EnvMarkers {
    present: false,
    non_empty: false,
};
pub const EMPTY_MARKER: EnvMarkers = EnvMarkers {
    present: true,
    non_empty: false,
};
pub const FULL_MARKER: EnvMarkers = EnvMarkers {
    present: true,
    non_empty: true,
};

pub fn task_id(n: u32) -> TaskId {
    TaskId::new(format!("{n:08x}-0000-4000-8000-000000000000")).unwrap()
}

pub fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1_000 + u64::from(pid), 5, BOOT.to_string()).unwrap()
}

pub fn root() -> StateRoot {
    StateRoot::from_normalized("/state".to_string())
}

pub fn facts(n: u32, status: &str, exit: Option<i64>) -> TaskFacts {
    TaskFacts {
        id: task_id(n),
        backend: "claude".into(),
        repo_path: "/repo".into(),
        session_id: "s".into(),
        status: status.into(),
        exit_code: exit,
        started_at: "2026-01-01T00:00:00+00:00".into(),
        started_at_us: i64::from(n) * 1_000_000,
        finished_at: None,
        finished_at_us: None,
        output_drained: false,
        title: None,
        model: None,
        continuation_of: None,
        chain_root: ChainRoot::Absent,
    }
}

pub fn record(n: u32, status: &str, exit: Option<i64>, pid: Option<u32>) -> TaskRecord {
    TaskRecord {
        facts: facts(n, status, exit),
        header: br#"{"task_id":"x"}"#.to_vec(),
        status_blob: None,
        process: pid.map(|pid| ProcessRecord {
            pid,
            start: Some(identity(pid)),
        }),
    }
}

pub fn caller(n: u32, pid: u32) -> CallerRow {
    CallerRow {
        task: task_id(n),
        status: "running".into(),
        exit_code: None,
        process: ProcessRecord {
            pid,
            start: Some(identity(pid)),
        },
    }
}

pub struct World {
    pub class: RootClass,
    pub view: Result<ProcessView, ViewFailure>,
    pub seen: HashMap<u32, PidObservation>,
    pub tasks: Vec<TaskRecord>,
    pub callers: Vec<CallerRow>,
    pub damaged: bool,
    pub pending: Vec<TaskId>,
    /// Ids that look up as found but have no record in the snapshot.
    pub vanish: Vec<TaskId>,
    /// The stop row served with a task's record; a task not listed has none.
    pub stops: Vec<(TaskId, hf_domain::ctl::continuation_stop::StopRow)>,
    pub unobserved_total: Option<u64>,
    pub open_error: Option<StoreFailure>,
    pub log: RefCell<Vec<String>>,
    pub history: History,
    pub feed: FeedWorld,
}

impl World {
    pub fn new() -> Self {
        Self {
            class: RootClass::MarkedStore,
            view: Ok(ProcessView {
                pid: 500,
                session: 77,
                ancestors: vec![400, 300],
            }),
            seen: HashMap::new(),
            tasks: Vec::new(),
            callers: Vec::new(),
            damaged: false,
            pending: Vec::new(),
            vanish: Vec::new(),
            stops: Vec::new(),
            unobserved_total: None,
            open_error: None,
            log: RefCell::new(Vec::new()),
            history: History::default(),
            feed: FeedWorld::default(),
        }
    }

    pub fn reads(&self, markers: EnvMarkers) -> Reads<'_> {
        Reads {
            roots: self,
            store: self,
            process: self,
            markers,
            deadline: Instant::now() + std::time::Duration::from_secs(600),
        }
    }

    pub fn calls(&self, prefix: &str) -> usize {
        self.log
            .borrow()
            .iter()
            .filter(|entry| entry.starts_with(prefix))
            .count()
    }

    pub fn order(&self) -> Vec<String> {
        self.log
            .borrow()
            .iter()
            .map(|entry| entry.split(':').next().unwrap_or_default().to_string())
            .collect()
    }

    pub(crate) fn note(&self, entry: String) {
        self.log.borrow_mut().push(entry);
    }
}

impl RootPort for World {
    fn classify(&self, _root: &StateRoot) -> RootClass {
        self.note("classify".into());
        self.class.clone()
    }
}

impl ProcessViewPort for World {
    fn view(&self, _deadline: Instant) -> Result<ProcessView, ViewFailure> {
        self.note("view".into());
        self.view.clone()
    }
    fn observe(&self, pid: u32) -> PidObservation {
        self.note(format!("observe:{pid}"));
        self.seen
            .get(&pid)
            .cloned()
            .unwrap_or(PidObservation::Absent)
    }
}

impl TaskStorePort for World {
    fn with_snapshot(
        &self,
        _root: &StateRoot,
        _deadline: Instant,
        action: &mut dyn FnMut(&dyn TaskSnapshotPort) -> Result<(), ReadError>,
    ) -> Result<(), ReadError> {
        self.note("snapshot".into());
        if let Some(failure) = &self.open_error {
            return Err(ReadError::Store(failure.clone()));
        }
        action(self)
    }
}

/// The bytes a view holds: its stored blobs and the typed text the renderer adds to them.
pub fn real_size(view: &hf_application::ctl::task_reads::TaskView) -> usize {
    let facts = &view.record.facts;
    view.record.header.len()
        + view.record.status_blob.as_ref().map_or(0, Vec::len)
        + facts.repo_path.len()
        + facts.session_id.len()
        + facts.started_at.len()
        + facts.title.as_ref().map_or(0, String::len)
        + facts.model.as_ref().map_or(0, String::len)
        + 200
}

/// The page read with a real item weight, the only path now that `page` is gone.
pub fn page(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &hf_application::ctl::task_reads::PageRequest,
) -> Result<hf_application::ctl::task_reads::PageOut, ReadError> {
    hf_application::ctl::task_reads::page_with(reads, root, request, &real_size)
}

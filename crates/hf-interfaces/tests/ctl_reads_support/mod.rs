#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! A scripted world behind the task-read ports, rendered through `dispatch`.
mod feed;
mod history;
mod pending;
pub mod pending_cursor;
#[allow(unused_imports)]
pub use feed::{Feeds, facts};
use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        ProcessViewPort, ReadError, Reads, RootPort, StoreFailure, TaskSnapshotPort, TaskStorePort,
        ViewFailure,
    },
};
use hf_domain::{
    ctl::{
        caller::{EnvMarkers, PidObservation, ProcessView},
        identity::HostIdentity,
        inspect::StateInspection,
        state_root::{InvalidStateRoot, StateRoot},
        task_rows::{
            BatchQuery, CallerRow, ChainRoot, ProcessRecord, TaskFacts, TaskRecord, TaskSummary,
            UnobservedRow,
        },
    },
    process::ProcessIdentity,
    task_read::{Lookup, TaskId},
};
use hf_interfaces::ctl::dispatch::{CtlServices, dispatch};
#[allow(unused_imports)]
pub use history::{History, Targets, event_row, page_of};
pub use pending::PendingScript;
use serde_json::Value;
use std::time::Instant;

pub const OTHER: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub const ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";

pub fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub struct Fake {
    pub status: String,
    pub exit: Option<i64>,
    pub seen: PidObservation,
    pub parent: Option<ProcessIdentity>,
    pub damaged: bool,
    pub header_extra: &'static str,
    pub clock: bool,
    /// The reader clock when `clock` is set, in microseconds since the epoch.
    pub now_us: i64,
    /// The stored chain root of the task and the stop row served with its record.
    pub chain_root: ChainRoot,
    pub stop: hf_domain::ctl::continuation_stop::StopRow,
    /// A continuation link to a related task whose header is `related_extra`.
    pub related_extra: Option<&'static str>,
    pub history: History,
    /// Related caller rows the gate sees.
    pub related: Vec<CallerRow>,
    pub targets: Targets,
    pub artifact_support: bool,
    pub root_marker: bool,
    /// Members added to the status blob, each as `,"key":value`.
    pub status_extra: &'static str,
    pub feed: Feeds,
    pub pending: PendingScript,
    /// The `(code, check)` of a launch receipt that does not qualify, served for every id.
    pub launch_refusal: Option<(&'static str, &'static str)>,
    /// The value of `HARFLOW_RETENTION_DAYS`.
    pub retention: Option<String>,
    /// The state root the services resolve.
    pub root: String,
}

pub fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1_700_000_000, 123_456, BOOT.to_string()).unwrap()
}

impl Fake {
    fn record(&self) -> TaskRecord {
        TaskRecord {
            facts: TaskFacts {
                id: TaskId::new(ID.to_string()).unwrap(),
                backend: "claude".into(),
                repo_path: "/r".into(),
                session_id: "s".into(),
                status: self.status.clone(),
                exit_code: self.exit,
                started_at: "2026-01-01T00:00:00+00:00".into(),
                started_at_us: 1_767_225_600_000_000,
                finished_at: None,
                finished_at_us: None,
                output_drained: false,
                title: None,
                model: None,
                continuation_of: self.related_extra.map(|_| TaskId::new(OTHER.to_string()).unwrap()),
                chain_root: self.chain_root.clone(),
            },
            header: format!(r#"{{"task_id":"{ID}"{}}}"#, self.header_extra).into_bytes(),
            status_blob: Some(format!(r#"{{"task_id":"{ID}","needs_reconciliation":true,"process_identity_state":"dead"{}}}"#, self.status_extra).into_bytes()),
            process: Some(ProcessRecord { pid: 40, start: Some(identity(40)) }),
        }
    }
}

impl RootPort for Fake {
    fn classify(&self, _: &StateRoot) -> RootClass {
        RootClass::MarkedStore
    }
}
impl ProcessViewPort for Fake {
    fn view(&self, _: Instant) -> Result<ProcessView, ViewFailure> {
        Ok(ProcessView {
            pid: 9,
            session: 3,
            ancestors: vec![],
        })
    }
    fn observe(&self, _: u32) -> PidObservation {
        self.seen.clone()
    }
}
impl TaskStorePort for Fake {
    fn with_snapshot(
        &self,
        _: &StateRoot,
        _: Instant,
        action: &mut dyn FnMut(&dyn TaskSnapshotPort) -> Result<(), ReadError>,
    ) -> Result<(), ReadError> {
        action(self)
    }
}
impl TaskSnapshotPort for Fake {
    fn damaged(&self) -> Result<bool, StoreFailure> {
        Ok(self.damaged)
    }
    fn related_callers(
        &self,
        _: i64,
        _: &[u32],
        _: Option<usize>,
    ) -> Result<Vec<CallerRow>, StoreFailure> {
        Ok(self.related.clone())
    }
    fn lookup(&self, id: &TaskId) -> Result<Lookup, StoreFailure> {
        if let Some((code, check)) = self.launch_refusal {
            return Ok(Lookup::LaunchRefused {
                id: id.clone(),
                code,
                check,
            });
        }
        Ok(Lookup::Found(id.clone()))
    }
    fn summaries(&self, _: &[TaskId]) -> Result<Vec<TaskSummary>, StoreFailure> {
        Ok(vec![])
    }
    fn record(&self, id: &TaskId) -> Result<Option<TaskRecord>, StoreFailure> {
        if id.as_str() == OTHER {
            let mut other = self.record();
            other.facts.id = id.clone();
            other.facts.continuation_of = None;
            other.header = format!(
                r#"{{"task_id":"{OTHER}"{}}}"#,
                self.related_extra.unwrap_or("")
            )
            .into_bytes();
            other.status_blob = Some(format!(r#"{{"task_id":"{OTHER}"}}"#).into_bytes());
            return Ok(Some(other));
        }
        Ok(Some(self.record()))
    }
    fn record_with_stop(
        &self,
        id: &TaskId,
    ) -> Result<Option<(TaskRecord, hf_domain::ctl::continuation_stop::StopRow)>, StoreFailure>
    {
        let row = if id.as_str() == OTHER {
            hf_domain::ctl::continuation_stop::StopRow::Absent
        } else {
            self.stop.clone()
        };
        Ok(TaskSnapshotPort::record(self, id)?.map(|record| (record, row)))
    }
    fn batch(&self, q: &BatchQuery) -> Result<Vec<TaskRecord>, StoreFailure> {
        Ok(if q.after.is_some() {
            vec![]
        } else {
            vec![self.record()]
        })
    }
    fn unobserved_count(&self) -> Result<u64, StoreFailure> {
        Ok(0)
    }
    fn unobserved_rows(
        &self,
        _: Option<(i64, String)>,
        _: usize,
    ) -> Result<Vec<UnobservedRow>, StoreFailure> {
        Ok(vec![])
    }
    fn cursor_scope(
        &self,
        _: bool,
        _: Option<&str>,
        _: Option<&TaskId>,
    ) -> Result<String, StoreFailure> {
        Ok("0".repeat(64))
    }
}
impl CtlServices for Fake {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot> {
        Ok(StateRoot::from_normalized(self.root.clone()))
    }
    fn host_identity(&self) -> HostIdentity {
        HostIdentity::from_rust("macos", "aarch64")
    }
    fn inspect(&self, root: &StateRoot) -> StateInspection {
        StateInspection::new(
            root,
            hf_domain::ctl::inspect::InspectStatus::Ready,
            hf_domain::ctl::inspect::InspectReason::Compatible,
            vec![],
        )
    }
    fn store_reads_supported(&self) -> bool {
        true
    }
    fn reads(&self) -> Option<Reads<'_>> {
        Some(Reads {
            roots: self,
            store: self,
            process: self,
            markers: EnvMarkers::default(),
            deadline: Instant::now() + std::time::Duration::from_secs(600),
        })
    }
    fn now_micros(&self) -> Option<i64> {
        self.clock.then_some(self.now_us)
    }
    fn pending_limits(&self) -> hf_application::ctl::task_reads::PendingLimits {
        self.pending.limits
    }
    fn retention_days(&self) -> Option<String> {
        self.retention.clone()
    }
    fn observer_parent(&self) -> Option<ProcessIdentity> {
        self.parent.clone()
    }
    fn artifact_targets(&self) -> Option<&dyn hf_application::ctl::task_reads::ArtifactTargetPort> {
        self.artifact_support
            .then_some(&self.targets as &dyn hf_application::ctl::task_reads::ArtifactTargetPort)
    }
    fn root_task_marker(&self) -> bool {
        self.root_marker
    }
}

pub fn fake(status: &str, exit: Option<i64>, seen: PidObservation) -> Fake {
    Fake {
        status: status.into(),
        exit,
        seen,
        parent: Some(identity(77)),
        damaged: false,
        header_extra: "",
        clock: true,
        now_us: 1_800_000_000_000_000,
        chain_root: ChainRoot::Absent,
        stop: hf_domain::ctl::continuation_stop::StopRow::Absent,
        related_extra: None,
        history: History::default(),
        related: Vec::new(),
        targets: Targets::default(),
        artifact_support: true,
        root_marker: false,
        status_extra: "",
        feed: Feeds::default(),
        pending: PendingScript::default(),
        launch_refusal: None,
        retention: None,
        root: "/s".into(),
    }
}

pub fn task(fake: &Fake, extra: &[&str]) -> Value {
    let mut list = vec!["status", ID, "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["task"].clone()
}

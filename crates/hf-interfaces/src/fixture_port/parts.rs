//! Event, artifact and live-caller requests.
use super::{
    PortError,
    args::Args,
    spec::{PromptStore, TaskSpec, task_id},
};
use hf_domain::{task_read::TaskId, timestamp::parse_micros};
use serde_json::{Map, Value, json};
const RESERVED: [&str; 8] = [
    "product",
    "format",
    "v",
    "seq",
    "kind",
    "observed_at",
    "task_id",
    "product_version",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSpec {
    pub seq: u64,
    pub kind: String,
    pub observed_at: String,
    fields: Map<String, Value>,
}
impl EventSpec {
    pub(super) fn parse(value: Value) -> Result<Self, PortError> {
        let args = Args::new(
            value,
            "event",
            &["seq", "kind", "observed_at", "fields"],
            &["seq", "kind", "observed_at", "fields"],
        )?;
        Self::from_args(&args)
    }
    pub(super) fn from_args(args: &Args) -> Result<Self, PortError> {
        let fields = args.object("fields")?.clone();
        if let Some(key) = fields.keys().find(|key| RESERVED.contains(&key.as_str())) {
            return Err(PortError::new(format!("event field {key:?} is reserved")));
        }
        Ok(Self {
            seq: args.u64("seq")?,
            kind: args.string("kind")?,
            observed_at: args.string("observed_at")?,
            fields,
        })
    }
    /// The stored line without its newline. This is the engine's internal normalized
    /// line, not the Python log format.
    pub fn body(&self, task: &TaskId) -> Vec<u8> {
        let mut line = Map::new();
        line.insert("product".into(), json!("harflow"));
        line.insert("format".into(), json!("harflow.event"));
        line.insert("v".into(), json!(1));
        line.insert("seq".into(), json!(self.seq));
        line.insert("kind".into(), json!(self.kind));
        line.insert("observed_at".into(), json!(self.observed_at));
        line.insert("task_id".into(), json!(task.as_str()));
        line.extend(self.fields.clone());
        serde_json::to_vec(&Value::Object(line)).unwrap_or_default()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Present,
    Absent(Observation),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactSpec {
    pub artifact_id: String,
    pub name: String,
    pub target: Target,
    pub kind: String,
    pub note: String,
    pub registered_at: String,
    pub provenance_method: String,
}
const ARTIFACT_KEYS: [&str; 7] = [
    "artifact_id",
    "name",
    "target",
    "kind",
    "note",
    "registered_at",
    "provenance_method",
];
impl ArtifactSpec {
    pub(super) fn parse(value: Value) -> Result<Self, PortError> {
        let mut keys = ARTIFACT_KEYS.to_vec();
        keys.push("observation");
        let mut args = Args::new(value, "artifact item", &keys, &ARTIFACT_KEYS)?;
        let name = args.string("name")?;
        check_name(&name)?;
        let target = match args.string("target")?.as_str() {
            "present" if args.has("observation") => {
                return Err(PortError::new("a present artifact carries no observation"));
            }
            "present" => Target::Present,
            "absent" => {
                let observation = Args::new(
                    args.take("observation"),
                    "observation",
                    &["device", "inode", "size", "mtime_ns"],
                    &["device", "inode", "size", "mtime_ns"],
                )?;
                Target::Absent(Observation {
                    device: observation.u64("device")?,
                    inode: observation.u64("inode")?,
                    size: observation.u64("size")?,
                    mtime_ns: observation.u64("mtime_ns")?,
                })
            }
            other => return Err(PortError::new(format!("unknown artifact target {other:?}"))),
        };
        Ok(Self {
            artifact_id: args.string("artifact_id")?,
            name,
            target,
            kind: args.string("kind")?,
            note: args.string("note")?,
            registered_at: args.string("registered_at")?,
            provenance_method: args.string("provenance_method")?,
        })
    }
    /// `<repo_path>/out/<name>`, the port's artifact location.
    pub fn path(&self, repo_path: &str) -> String {
        format!("{repo_path}/out/{}", self.name)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityState {
    Alive,
    Reused,
    Unknown,
    /// A Python-record form, or a spelling this engine does not know.
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallerStatus {
    Running,
    CompletedObserved,
    FailedUnobserved,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveCaller {
    pub id: TaskId,
    pub pid: u32,
    pub identity: IdentityState,
    pub status: CallerStatus,
}
impl LiveCaller {
    pub(super) fn parse(args: &Args) -> Result<Self, PortError> {
        let pid =
            u32::try_from(args.u64("pid")?).map_err(|_| PortError::new("pid out of range"))?;
        let identity = match args.string("identity")?.as_str() {
            "alive" => IdentityState::Alive,
            "reused" => IdentityState::Reused,
            "unknown" => IdentityState::Unknown,
            _ => IdentityState::Unsupported,
        };
        let status = match args.string("status")?.as_str() {
            "running" => CallerStatus::Running,
            "completed_observed" => CallerStatus::CompletedObserved,
            "failed_unobserved" => CallerStatus::FailedUnobserved,
            other => return Err(PortError::new(format!("unknown caller status {other:?}"))),
        };
        args.opt_string("marker")?;
        Ok(Self {
            id: task_id(args.string("task_id")?)?,
            pid,
            identity,
            status,
        })
    }
    /// The task record the port gives a caller, per the Python seed's defaults.
    pub fn task_spec(&self) -> TaskSpec {
        let (status, exit_code, finished, drained) = match self.status {
            CallerStatus::Running => ("running", None, None, false),
            CallerStatus::CompletedObserved => (
                "completed",
                Some(0),
                Some("2026-11-07T00:01:00+00:00"),
                true,
            ),
            CallerStatus::FailedUnobserved => {
                ("failed", None, Some("2026-11-07T00:01:00+00:00"), false)
            }
        };
        let session: String = self.id.as_str().chars().take(8).collect();
        TaskSpec {
            id: self.id.clone(),
            backend: "claude".into(),
            repo_path: "/workspace/repo".into(),
            freedom: "write_in_repo".into(),
            session_id: format!("session-{session}"),
            status: status.into(),
            exit_code,
            started_at: "2026-11-07T00:00:00+00:00".into(),
            started_at_us: micros("2026-11-07T00:00:00+00:00"),
            finished_at: finished.map(str::to_owned),
            finished_at_us: finished.map(micros),
            output_drained: drained,
            prompt: "the caller".into(),
            title: None,
            group: None,
            model: None,
            continuation_of: None,
            prompt_store: PromptStore::Inline,
            real_workspace: false,
        }
    }
    pub fn process_identity_state(&self) -> &'static str {
        match self.identity {
            IdentityState::Alive => "alive",
            IdentityState::Reused => "dead",
            _ => "uncertain",
        }
    }
}
fn micros(text: &str) -> i64 {
    parse_micros(text).unwrap_or_default()
}
/// An artifact name is one path component, so a target always lies under `<repo>/out/`.
pub fn check_name(name: &str) -> Result<(), PortError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(PortError::new("artifact name must be one path component"));
    }
    Ok(())
}

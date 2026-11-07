//! The `add_task` spec, the served header and status blobs, and the prompt record.
use super::{PortError, args::Args};
use hf_domain::{task_read::TaskId, timestamp::parse_micros};
use serde_json::{Value, json};
pub const PREVIEW_CHARS: usize = 2000;
const KEYS: [&str; 17] = [
    "task_id",
    "backend",
    "repo_path",
    "freedom",
    "session_id",
    "status",
    "exit_code",
    "started_at",
    "finished_at",
    "output_drained",
    "prompt",
    "title",
    "group",
    "model",
    "continuation_of",
    "prompt_store",
    "real_workspace",
];
const REQUIRED: [&str; 11] = [
    "task_id",
    "backend",
    "repo_path",
    "freedom",
    "session_id",
    "status",
    "exit_code",
    "started_at",
    "finished_at",
    "output_drained",
    "prompt",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptStore {
    Inline,
    SidecarVerified,
    SidecarMissing,
    SidecarMismatch,
    SidecarForeignName,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: TaskId,
    pub backend: String,
    pub repo_path: String,
    pub freedom: String,
    pub session_id: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub started_at_us: i64,
    pub finished_at: Option<String>,
    pub finished_at_us: Option<i64>,
    pub output_drained: bool,
    pub prompt: String,
    pub title: Option<String>,
    pub group: Option<String>,
    pub model: Option<String>,
    pub continuation_of: Option<TaskId>,
    pub prompt_store: PromptStore,
    pub real_workspace: bool,
}
/// The prompt as stored: inline text, or a preview with its sidecar binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRecord {
    pub inline_text: Option<String>,
    pub preview: Option<String>,
    pub claimed_sha256: Option<String>,
    pub source_name: Option<String>,
    pub blob: Option<Vec<u8>>,
}
/// Fields that exist only to give recovery, observation and ownership facts a home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Projection {
    pub process_identity_state: &'static str,
    pub needs_reconciliation: bool,
}
pub(super) fn task_id(text: String) -> Result<TaskId, PortError> {
    TaskId::new(text).map_err(|_| PortError::new("invalid task id"))
}
impl TaskSpec {
    pub(super) fn parse(value: Value) -> Result<Self, PortError> {
        let args = Args::new(value, "task spec", &KEYS, &REQUIRED)?;
        let started_at = args.string("started_at")?;
        let finished_at = args.opt_string("finished_at")?;
        let micros = |text: &str| {
            parse_micros(text)
                .ok_or_else(|| PortError::new(format!("unparseable timestamp {text:?}")))
        };
        let finished_at_us = finished_at.as_deref().map(micros).transpose()?;
        let id = task_id(args.string("task_id")?)?;
        let continuation_of = args
            .opt_string("continuation_of")?
            .filter(|text| !text.is_empty())
            .map(task_id)
            .transpose()?;
        Ok(Self {
            started_at_us: micros(&started_at)?,
            started_at,
            finished_at,
            finished_at_us,
            id,
            backend: args.string("backend")?,
            repo_path: args.string("repo_path")?,
            freedom: args.string("freedom")?,
            session_id: args.string("session_id")?,
            status: args.string("status")?,
            exit_code: args.opt_i64("exit_code")?,
            output_drained: args.bool("output_drained")?,
            prompt: args.string("prompt")?,
            title: args.opt_string("title")?,
            group: args.opt_string("group")?,
            model: args.opt_string("model")?,
            continuation_of,
            prompt_store: store(args.opt_string("prompt_store")?.as_deref())?,
            real_workspace: args.opt_bool("real_workspace")?.unwrap_or(false),
        })
    }
    /// The identity fields of the task: what was asked for, not how it went.
    pub fn header_blob(&self) -> Vec<u8> {
        encode(&json!({
            "task_id": self.id.as_str(),
            "backend": self.backend,
            "repo_path": self.repo_path,
            "freedom": self.freedom,
            "session_id": self.session_id,
            "started_at": self.started_at,
            "title": self.title,
            "group": self.group,
            "model": self.model,
            "continuation_of_task_id": self.continuation_of.as_ref().map(TaskId::as_str),
            "chain_root_task_id": self.continuation_of.as_ref().map(TaskId::as_str),
        }))
    }
    /// The projection of a task. `identity` is the decided state of a recorded caller process
    /// (`alive`, `dead` or `uncertain`); without one an unobserved outcome is `uncertain`. An
    /// observed outcome (not running, with an exit code) is always `dead`, as in the Python store's indexed status.
    pub fn projection(&self, identity: Option<&'static str>) -> Projection {
        // Python's `outcome_unobserved`: a running status or a missing exit code.
        if self.status != "running" && self.exit_code.is_some() {
            return Projection {
                process_identity_state: "dead",
                needs_reconciliation: false,
            };
        }
        let state = identity.unwrap_or("uncertain");
        let terminal =
            ["cancelled", "completed", "timed_out", "failed"].contains(&self.status.as_str());
        let needs_reconciliation = match state {
            "alive" => terminal,
            "dead" => !["cancelled", "completed", "timed_out"].contains(&self.status.as_str()),
            _ => true,
        };
        Projection {
            process_identity_state: state,
            needs_reconciliation,
        }
    }
    /// How the task went, with the observation projection fields.
    pub fn status_blob(&self, projection: Projection) -> Vec<u8> {
        encode(&json!({
            "task_id": self.id.as_str(),
            "status": self.status,
            "exit_code": self.exit_code,
            "finished_at": self.finished_at,
            "output_drained": self.output_drained,
            "recovered": true,
            "observation_mode": "unknown",
            "needs_reconciliation": projection.needs_reconciliation,
            "owner": null,
            "phase_slots": [],
            "process_identity_state": projection.process_identity_state,
        }))
    }
    /// `digest` returns lowercase hexadecimal SHA-256 of its input.
    pub fn prompt_record(
        &self,
        digest: &dyn Fn(&[u8]) -> Result<String, PortError>,
    ) -> Result<PromptRecord, PortError> {
        let text = &self.prompt;
        let preview = || text.chars().take(PREVIEW_CHARS).collect::<String>();
        let named = |hash: &str| format!("{}.prompt.{hash}.txt", self.id.as_str());
        let sidecar =
            |claimed: Option<String>, source: String, blob: Option<Vec<u8>>| PromptRecord {
                inline_text: None,
                preview: Some(preview()),
                claimed_sha256: claimed,
                source_name: Some(source),
                blob,
            };
        let true_hash = digest(text.as_bytes())?;
        Ok(match self.prompt_store {
            PromptStore::Inline => PromptRecord {
                inline_text: Some(text.clone()),
                preview: None,
                claimed_sha256: None,
                source_name: None,
                blob: None,
            },
            PromptStore::SidecarVerified => sidecar(
                Some(true_hash.clone()),
                named(&true_hash),
                Some(text.clone().into_bytes()),
            ),
            PromptStore::SidecarMismatch => {
                let tampered = format!("{text}tampered").into_bytes();
                sidecar(Some(true_hash.clone()), named(&true_hash), Some(tampered))
            }
            PromptStore::SidecarMissing => {
                let absent = digest(b"absent")?;
                sidecar(Some(absent.clone()), named(&absent), None)
            }
            PromptStore::SidecarForeignName => sidecar(None, "bad-name.txt".to_owned(), None),
        })
    }
}
fn store(text: Option<&str>) -> Result<PromptStore, PortError> {
    Ok(match text {
        None | Some("inline") => PromptStore::Inline,
        Some("sidecar_verified") => PromptStore::SidecarVerified,
        Some("sidecar_missing") => PromptStore::SidecarMissing,
        Some("sidecar_mismatch") => PromptStore::SidecarMismatch,
        Some("sidecar_foreign_name") => PromptStore::SidecarForeignName,
        Some(other) => return Err(PortError::new(format!("unknown prompt_store {other:?}"))),
    })
}
fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}
/// The `repo_path` of a header blob written by `TaskSpec::header_blob`.
pub fn header_repo_path(header: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(header).ok()?;
    Some(value.get("repo_path")?.as_str()?.to_owned())
}

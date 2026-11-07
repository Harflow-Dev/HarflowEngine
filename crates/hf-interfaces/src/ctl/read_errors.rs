//! Error documents of the task reads: code, message and data, per path.
use super::cli_scan::py_repr;
use hf_application::ctl::task_reads::{ReadError, Resolution, StoreFailure, UnknownReason};
use hf_domain::ctl::{caller::RefusalKind, identity::PRODUCT_VERSION, inspect::InspectReason};
use serde_json::{Map, Value, json};

/// Which refusal vocabulary a path uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadPath {
    /// `status`, `status-batch` and `list`.
    Managed,
    /// The plain page read.
    Page,
    /// Successor discovery, a direct-only read.
    Successors,
    /// `task-events` and `task-prompt`, direct-only reads of one task's history.
    History,
    /// `task-artifacts`: every failure is an artifact error.
    Artifacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorDoc {
    pub code: &'static str,
    pub message: String,
    pub data: Option<Value>,
}

impl ErrorDoc {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// The `error` object of a document, and an entry of a batch's `errors`.
    pub fn object(&self) -> Value {
        let mut map = Map::new();
        map.insert("code".into(), json!(self.code));
        map.insert("message".into(), json!(self.message));
        if let Some(data) = &self.data {
            map.insert("data".into(), data.clone());
        }
        Value::Object(map)
    }
}

/// The shape every state error carries.
pub fn state_data(state_root: &str, reason: &str) -> Value {
    json!({
        "engine_version": PRODUCT_VERSION,
        "reason": reason,
        "recovery_supported": false,
        "state_root": state_root,
    })
}

pub fn inconsistent(state_root: &str) -> ErrorDoc {
    let mut doc = ErrorDoc::new("state_incompatible", "State root cannot be read");
    doc.data = Some(state_data(
        state_root,
        InspectReason::InfrastructureUnavailable.as_str(),
    ));
    doc
}

fn refusal(path: ReadPath, kind: RefusalKind) -> ErrorDoc {
    let who = match kind {
        RefusalKind::Agent => "the caller is an agent whose workflow ownership cannot be resolved",
        RefusalKind::Undecidable => "caller detection could not establish who is calling",
    };
    match (path, kind) {
        (ReadPath::Managed, _) => ErrorDoc::new(
            "workflow_read_refused",
            format!("Workflow task reads are refused: {who}"),
        ),
        (ReadPath::Page, _) => {
            ErrorDoc::new("invalid_params", format!("Task listing is refused: {who}"))
        }
        (ReadPath::Successors, RefusalKind::Agent) => ErrorDoc::new(
            "agent_caller",
            format!("Successor discovery requires a verified direct caller: {who}"),
        ),
        (ReadPath::Successors, RefusalKind::Undecidable) => ErrorDoc::new(
            "caller_undecidable",
            format!("Successor discovery requires a verified direct caller: {who}"),
        ),
        (ReadPath::History, RefusalKind::Agent) => ErrorDoc::new(
            "agent_caller",
            format!("Task history reads require a verified direct caller: {who}"),
        ),
        (ReadPath::History, RefusalKind::Undecidable) => ErrorDoc::new(
            "caller_undecidable",
            format!("Task history reads require a verified direct caller: {who}"),
        ),
        (ReadPath::Artifacts, _) => ErrorDoc::new(
            "artifact_error",
            format!("Task artifacts are refused: {who}"),
        ),
    }
}

/// An event cursor refusal: the client reloads the newest page.
fn cursor(code: &'static str, message: &str) -> ErrorDoc {
    let mut doc = ErrorDoc::new(code, format!("{message}; reload the newest page"));
    doc.data = Some(json!({"reload": "newest"}));
    doc
}

/// What an artifact read says about its own failures: all of them are artifact errors.
fn artifact_failure(error: &ReadError) -> Option<ErrorDoc> {
    match error {
        ReadError::InvalidTaskId(raw) => Some(ErrorDoc::new(
            "artifact_error",
            format!("not a valid task id: {}", py_repr(raw)),
        )),
        ReadError::InvalidParams(message) => Some(ErrorDoc::new("artifact_error", message.clone())),
        _ => None,
    }
}

fn resolution(found: &Resolution) -> ErrorDoc {
    match found {
        Resolution::Unknown { input, reason } => {
            let head = format!("unknown task_id: {input}");
            let (message, extra) = match reason {
                None => (
                    format!("{head}. Use list_tasks to see what is known."),
                    Vec::new(),
                ),
                Some(UnknownReason::PrefixTooShort) => (
                    format!(
                        "{head}. It is not an existing task id, and a prefix must be at least 8 characters to be resolved."
                    ),
                    vec![
                        ("reason", json!("prefix_too_short")),
                        ("min_prefix_length", json!(8)),
                    ],
                ),
                Some(UnknownReason::NoPrefixMatch) => (
                    format!(
                        "{head}. No task id starts with it (unique prefixes of at least 8 characters are accepted)."
                    ),
                    vec![("reason", json!("no_prefix_match"))],
                ),
                Some(UnknownReason::PrefixIndexUnavailable) => (
                    format!(
                        "{head}. The index needed to resolve a prefix is not ready; retry, or pass the full id."
                    ),
                    vec![("reason", json!("prefix_index_unavailable"))],
                ),
            };
            let mut data = Map::new();
            data.insert("code".into(), json!("unknown_task"));
            data.insert("input".into(), json!(input));
            for (key, value) in extra {
                data.insert(key.into(), value);
            }
            let mut doc = ErrorDoc::new("unknown_task", message);
            doc.data = Some(Value::Object(data));
            doc
        }
        Resolution::Ambiguous {
            input,
            candidates,
            truncated,
        } => {
            let count = if *truncated {
                "more than 5".to_string()
            } else {
                candidates.len().to_string()
            };
            let listed: Vec<Value> = candidates
                .iter()
                .map(|task| {
                    json!({
                        "task_id": task.task_id,
                        "title": task.title,
                        "status": task.status,
                        "created_at": task.created_at,
                    })
                })
                .collect();
            let mut doc = ErrorDoc::new(
                "ambiguous_task_id",
                format!(
                    "ambiguous task_id prefix '{input}' matches {count} tasks; use a longer prefix or the full id"
                ),
            );
            doc.data = Some(json!({
                "code": "ambiguous_task_id",
                "input": input,
                "candidates": listed,
                "truncated": truncated,
            }));
            doc
        }
    }
}

/// `state_root` is the selected root, named in state errors.
pub fn describe(error: &ReadError, path: ReadPath, state_root: &str) -> ErrorDoc {
    if path == ReadPath::Artifacts
        && let Some(doc) = artifact_failure(error)
    {
        return doc;
    }
    match error {
        ReadError::CursorInvalid => cursor("event_cursor_invalid", "Invalid event cursor"),
        ReadError::CursorExpired => cursor(
            "event_cursor_expired",
            "Event cursor generation is no longer served",
        ),
        ReadError::Store(StoreFailure::Busy) => {
            ErrorDoc::new("state_busy", "State is busy; retry the read")
        }
        ReadError::Store(StoreFailure::Incompatible { reason, message }) => {
            let mut doc = ErrorDoc::new("state_incompatible", message.clone());
            doc.data = Some(state_data(state_root, reason.as_str()));
            doc
        }
        ReadError::State(inspection) => {
            let message = inspection.reason.message(inspection.status);
            let mut doc = ErrorDoc::new("state_incompatible", message);
            doc.data = Some(state_data(
                &inspection.state_root,
                inspection.reason.as_str(),
            ));
            doc
        }
        ReadError::Refused(kind) => refusal(path, *kind),
        ReadError::Damaged => refusal(path, RefusalKind::Undecidable),
        ReadError::Internal => inconsistent(state_root),
        ReadError::InvalidTaskId(raw) => ErrorDoc::new(
            "invalid_task_id",
            format!("not a valid task id: {}", py_repr(raw)),
        ),
        ReadError::InvalidParams(message) => ErrorDoc::new("invalid_params", message.clone()),
        ReadError::Resolution(found) => resolution(found),
        ReadError::PendingLaunch(raw) => ErrorDoc::new(
            "task_pending_launch",
            format!(
                "task {raw} has not started: its launch is pending; no task record exists until it spawns"
            ),
        ),
    }
}

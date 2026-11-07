//! `status --brief`: the bounded display projection of a status document. It keeps an
//! allow-list of scalar fields, cuts text to display lengths, adds detail references and
//! trims the whole document to 8 KiB under `ensure_ascii` sizing. Fields with no source
//! in this engine stay absent.
use super::{bound::python_size, brief_outcome::project};
use hf_domain::task_read::TaskId;
use serde_json::{Map, Value, json};

const BRIEF_BYTES: usize = 8192;
/// Dropped largest first when no text is left to halve: the reference's three, then the
/// objects it copies unbounded.
const OPTIONAL: [&str; 6] = [
    "recent_activity",
    "outcome",
    "failure_diagnostic",
    "supervision",
    "successor_tasks",
    "phase_slots",
];
const TEXT_BYTES: usize = 1024;
const SCALAR_BYTES: usize = 256;
const ACTIVITY_BYTES: usize = 160;
const LIST_FIELDS: [&str; 29] = [
    "task_id",
    "backend",
    "status",
    "title",
    "group",
    "repo_path",
    "started_at",
    "finished_at",
    "session_id",
    "parent_task_id",
    "root_task_id",
    "duration_seconds",
    "background_idle_seconds",
    "output_capture",
    "input_channel",
    "last_captured_output_at",
    "recovered",
    "observation_mode",
    "observation_note",
    "owner",
    "dispatch_origin",
    "responding_server",
    "persisted_status",
    "observed_exit",
    "phase_slots",
    "needs_reconciliation",
    "process_identity_state",
    "needs_direct_lookup",
    "note",
];
const EXTRA_FIELDS: [&str; 4] = [
    "exit_code",
    "owned_by_live_server",
    "workflow_run_id",
    "workflow_node_id",
];

/// At most `maximum` bytes of the text, on a character boundary.
pub(super) fn text(value: &str, maximum: usize) -> String {
    let mut end = value.len().min(maximum);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn identity(value: Option<&Value>) -> Option<Value> {
    let map = value?.as_object()?;
    let pid = map.get("pid").filter(|pid| pid.is_i64() || pid.is_u64())?;
    let start = map
        .get("start_time")
        .and_then(Value::as_str)
        .map(|start| text(start, 128));
    Some(json!({"pid": pid, "start_time": start}))
}

/// Count and state of the artifact summary, with its detail reference kept only when it is
/// exactly the reference of this task.
fn artifacts(payload: &Map<String, Value>) -> Option<Value> {
    let summary = payload.get("artifact_summary")?.as_object()?;
    let count = summary
        .get("count")
        .and_then(Value::as_u64)
        .filter(|n| *n <= 100);
    let state = summary
        .get("state")
        .and_then(Value::as_str)
        .filter(|state| ["available", "unavailable"].contains(state));
    let mut projected = match (count, state) {
        (Some(count), Some(state)) => json!({"count": count, "state": state}),
        _ => json!({"count": 0, "state": "unavailable"}),
    };
    let task = payload.get("task_id").and_then(Value::as_str);
    let wanted = task.map(|id| json!({"tool": "get_task_artifacts", "task_id": id}));
    let reference = summary.get("detail_ref");
    if let (Some(id), Some(wanted), Some(map)) = (task, wanted, projected.as_object_mut())
        && TaskId::new(id.to_string()).is_ok()
        && reference == Some(&wanted)
    {
        map.insert("detail_ref".into(), wanted);
    }
    Some(projected)
}

/// The prose fields, cut to 1 KiB; the names of those that were cut.
fn prose(payload: &Map<String, Value>, result: &mut Map<String, Value>) -> Vec<Value> {
    let mut cut = Vec::new();
    for key in ["summary", "error", "note"] {
        let Some(original) = payload.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        let original = match original {
            Value::String(original) => original.clone(),
            other => other.to_string(),
        };
        let kept = text(&original, TEXT_BYTES);
        if kept != original {
            cut.push(json!(key));
        }
        result.insert(key.to_string(), Value::String(kept));
    }
    cut
}

fn size(result: &Map<String, Value>) -> usize {
    python_size(&Value::Object(result.clone()))
}

fn mark(result: &mut Map<String, Value>, key: &str) {
    if let Some(Value::Array(names)) = result.get_mut("truncated_fields")
        && !names.contains(&json!(key))
    {
        names.push(json!(key));
    }
}

/// Halves the longest text, or drops the largest optional list, until the document fits.
fn trim(result: &mut Map<String, Value>) {
    while size(result) > BRIEF_BYTES {
        let long = result
            .iter()
            .filter(|(_, value)| value.as_str().is_some_and(|s| s.chars().count() > 32))
            .max_by_key(|(_, value)| python_size(value))
            .map(|(key, _)| key.clone());
        let Some(key) = long else {
            // The reference stops at its three optional fields; the objects it copies whole
            // would leave the document over its bound, so they follow, never first.
            let largest = [&OPTIONAL[..3], &OPTIONAL[3..]]
                .iter()
                .find_map(|tier| {
                    tier.iter()
                        .filter(|key| result.contains_key(**key))
                        .max_by_key(|key| result.get(**key).map_or(0, python_size))
                })
                .map(|key| (*key).to_string());
            let Some(key) = largest else { break };
            result.remove(&key);
            mark(result, &key);
            continue;
        };
        if let Some(Value::String(value)) = result.get_mut(&key) {
            let half = value.chars().count() / 2;
            *value = value.chars().take(half).collect();
        }
        mark(result, &key);
    }
}

/// The brief of an already-authorized status document, with the recent activity lines.
pub fn brief_status(payload: &Map<String, Value>, recent: &[String]) -> Map<String, Value> {
    let scalar = |value: &Value| {
        value.is_null() || value.is_string() || value.is_boolean() || value.is_number()
    };
    let mut result = Map::new();
    for (key, value) in payload {
        let listed = LIST_FIELDS.contains(&key.as_str()) || EXTRA_FIELDS.contains(&key.as_str());
        if listed && scalar(value) {
            let kept = match value {
                Value::String(original) => Value::String(text(original, SCALAR_BYTES)),
                other => other.clone(),
            };
            result.insert(key.clone(), kept);
        }
    }
    if let Some(Value::Array(slots)) = payload.get("phase_slots") {
        result.insert(
            "phase_slots".into(),
            json!(slots.iter().take(1).collect::<Vec<_>>()),
        );
    }
    for key in ["supervision", "successor_tasks"] {
        if let Some(value) = payload.get(key).filter(|value| value.is_object()) {
            result.insert(key.to_string(), value.clone());
        }
    }
    for key in ["owner", "dispatch_origin", "responding_server"] {
        if let Some(value) = identity(payload.get(key)) {
            result.insert(key.to_string(), value);
        }
    }
    if let Some(summary) = artifacts(payload) {
        result.insert("artifact_summary".into(), summary);
    }
    let task = payload.get("task_id").map(shown).unwrap_or_default();
    let id = text(&task, 64);
    let mut truncated = prose(payload, &mut result);
    for (name, outcome) in [("outcome", true), ("failure_diagnostic", false)] {
        if project(payload, outcome, &mut result) {
            truncated.push(json!(name));
        }
    }
    let lines: Vec<Value> = recent
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|line| json!(text(line, ACTIVITY_BYTES)))
        .collect();
    result.insert("recent_activity".into(), Value::Array(lines));
    result.insert(
        "detail_refs".into(),
        json!({
            "status": {"tool": "get_task_status", "task_id": id},
            "events": {"tool": "get_task_events", "task_id": id},
        }),
    );
    result.insert("truncated_fields".into(), Value::Array(truncated));
    trim(&mut result);
    result
}

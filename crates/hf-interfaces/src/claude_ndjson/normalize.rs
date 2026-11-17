use super::Limits;
use hf_domain::{backend::*, ownership::Id};
use serde_json::Value;
/// Dynamic bytes count every returned string occurrence, including repeated provenance.
pub(super) fn records(
    v: &Value,
    attempt: &Id,
    limits: Limits,
    production: bool,
) -> Result<Vec<BackendRecord>, BackendError> {
    let session = string(v, "session_id", limits)?;
    let parent = match v.get("parent_tool_use_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if valid_string(s, limits) => Some(s.clone()),
        _ => return Err(BackendError::Shape),
    };
    let kinds = normalize(v, limits, production)?;
    if kinds.len() > limits.max_records {
        return Err(BackendError::Budget);
    }
    let mut bytes = 0usize;
    let mut records = Vec::new();
    for kind in kinds {
        let payload = match &kind {
            RecordKind::Text(s) | RecordKind::ToolUse(s) => s.len(),
            RecordKind::ToolResult { id, .. } => id.len(),
            _ => 0,
        };
        bytes = bytes
            .saturating_add(session.len())
            .saturating_add(attempt.as_str().len())
            .saturating_add(parent.as_ref().map_or(0, String::len))
            .saturating_add(payload);
        if bytes > limits.max_event_bytes {
            return Err(BackendError::Budget);
        }
        records.push(BackendRecord {
            cli_session: session.clone(),
            attempt: attempt.clone(),
            parent: parent.clone(),
            kind,
        });
    }
    Ok(records)
}
fn valid_string(s: &str, limits: Limits) -> bool {
    !s.is_empty() && s.len() <= limits.max_identifier_bytes && !s.chars().any(char::is_control)
}
fn string(v: &Value, key: &str, limits: Limits) -> Result<String, BackendError> {
    v[key]
        .as_str()
        .filter(|s| valid_string(s, limits))
        .map(String::from)
        .ok_or(BackendError::Shape)
}
fn normalize(v: &Value, limits: Limits, production: bool) -> Result<Vec<RecordKind>, BackendError> {
    if matches!(v["type"].as_str(), Some("system" | "result"))
        && !(production && v["type"] == "system" && v["subtype"] == "init")
    {
        reject_effects(v)?;
    }
    let kind = match v["type"].as_str() {
        Some("system") if v["subtype"].as_str() == Some("init") => RecordKind::Init,
        Some("system") if matches!(v["subtype"].as_str(), Some("status" | "compact_boundary")) => {
            RecordKind::Metadata
        }
        Some("system") if production && v["subtype"] == "post_turn_summary" => RecordKind::Metadata,
        Some("rate_limit_event") if production => {
            reject_effects(v)?;
            RecordKind::Metadata
        }
        Some("result") => {
            let error = v["is_error"].as_bool().ok_or(BackendError::Shape)?;
            let subtype = v["subtype"].as_str().ok_or(BackendError::Shape)?;
            if !matches!(
                subtype,
                "success"
                    | "error_during_execution"
                    | "error_max_turns"
                    | "error_max_budget_usd"
                    | "error_max_structured_output_retries"
            ) {
                return Err(BackendError::Unsupported);
            }
            RecordKind::Terminal {
                failed: error || subtype != "success",
            }
        }
        Some("assistant") => return content(&v["message"]["content"], true, limits),
        Some("user") => return content(&v["message"]["content"], false, limits),
        Some("stream_event") if production => {
            super::production_boundary(v)?;
            RecordKind::Metadata
        }
        _ => return Err(BackendError::Unsupported),
    };
    Ok(vec![kind])
}
fn content(v: &Value, assistant: bool, limits: Limits) -> Result<Vec<RecordKind>, BackendError> {
    let items = v
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or(BackendError::Shape)?;
    if items.len() > limits.max_records {
        return Err(BackendError::Budget);
    }
    let mut records = Vec::new();
    for item in items {
        let kind = match item["type"].as_str() {
            Some("text") if assistant => {
                RecordKind::Text(item["text"].as_str().ok_or(BackendError::Shape)?.into())
            }
            Some("tool_use") if assistant => RecordKind::ToolUse(string(item, "id", limits)?),
            Some("tool_result") if !assistant => RecordKind::ToolResult {
                id: string(item, "tool_use_id", limits)?,
                is_error: match item.get("is_error") {
                    None => false,
                    Some(v) => v.as_bool().ok_or(BackendError::Shape)?,
                },
            },
            _ => return Err(BackendError::Unsupported),
        };
        records.push(kind);
    }
    Ok(records)
}

fn reject_effects(v: &Value) -> Result<(), BackendError> {
    match v {
        Value::Object(fields) => {
            if fields
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|s| s.contains("tool") || s == "input_json_delta")
                || fields
                    .get("tools")
                    .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
            {
                return Err(BackendError::Unsupported);
            }
            for child in fields.values() {
                reject_effects(child)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                reject_effects(child)?;
            }
        }
        _ => (),
    }
    Ok(())
}

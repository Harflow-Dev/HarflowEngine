//! Independent bounded JSONL observations. A malformed suffix retains valid prefix records.
use hf_domain::{backend::*, ownership::Id};
use serde_json::Value;

pub struct ClaudeStreamDecoder {
    attempt: Id,
    pending: Vec<u8>,
    trace: BackendTrace,
    bytes: usize,
}
impl ClaudeStreamDecoder {
    pub fn new(attempt: Id) -> Self {
        Self {
            attempt,
            pending: Vec::new(),
            trace: BackendTrace {
                records: Vec::new(),
                fault: None,
                ended: false,
            },
            bytes: 0,
        }
    }
    pub fn push(&mut self, chunk: &[u8]) {
        if self.trace.fault.is_some() || self.trace.ended {
            return;
        }
        if chunk.len() > 4096 {
            self.trace.fault = Some(BackendError::Budget);
            return;
        }
        let permitted = chunk.len().min(65536 - self.bytes);
        self.bytes += permitted;
        for byte in &chunk[..permitted] {
            if *byte == b'\n' {
                if let Err(error) = self.line() {
                    self.trace.fault = Some(error);
                    return;
                }
                self.pending.clear();
            } else {
                if self.pending.len() >= 24576 {
                    self.trace.fault = Some(BackendError::Budget);
                    return;
                }
                self.pending.push(*byte);
            }
        }
        if permitted < chunk.len() {
            self.trace.fault = Some(BackendError::Budget);
        }
    }
    pub fn finish(mut self) -> BackendTrace {
        if !self.pending.is_empty() && self.trace.fault.is_none() {
            self.trace.fault = Some(BackendError::Truncated);
        }
        self.trace.ended = true;
        self.trace
    }
    fn line(&mut self) -> Result<(), BackendError> {
        let value = crate::owner_profile::decode_for_fixture(&self.pending)
            .map_err(|_| BackendError::Shape)?;
        let session = string(&value, "session_id")?;
        let parent = match value.get("parent_tool_use_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if valid_string(s) => Some(s.clone()),
            _ => return Err(BackendError::Shape),
        };
        let kinds = normalize(&value)?;
        if self.trace.records.len().saturating_add(kinds.len()) > 128 {
            return Err(BackendError::Budget);
        }
        for kind in kinds {
            self.trace.records.push(BackendRecord {
                cli_session: session.clone(),
                attempt: self.attempt.clone(),
                parent: parent.clone(),
                kind,
            });
        }
        Ok(())
    }
}
fn valid_string(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn string(v: &Value, key: &str) -> Result<String, BackendError> {
    v[key]
        .as_str()
        .filter(|s| valid_string(s))
        .map(String::from)
        .ok_or(BackendError::Shape)
}
fn normalize(v: &Value) -> Result<Vec<RecordKind>, BackendError> {
    if matches!(v["type"].as_str(), Some("system" | "result")) {
        reject_effects(v)?;
    }
    let kind = match v["type"].as_str() {
        Some("system") if v["subtype"].as_str() == Some("init") => RecordKind::Init,
        Some("system") if matches!(v["subtype"].as_str(), Some("status" | "compact_boundary")) => {
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
        Some("assistant") => return content(&v["message"]["content"], true),
        Some("user") => return content(&v["message"]["content"], false),
        _ => return Err(BackendError::Unsupported),
    };
    Ok(vec![kind])
}
fn content(v: &Value, assistant: bool) -> Result<Vec<RecordKind>, BackendError> {
    let items = v
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 128)
        .ok_or(BackendError::Shape)?;
    let mut records = Vec::new();
    for item in items {
        let kind = match item["type"].as_str() {
            Some("text") if assistant => RecordKind::Text(
                item["text"]
                    .as_str()
                    .filter(|s| s.len() <= 8192)
                    .ok_or(BackendError::Shape)?
                    .into(),
            ),
            Some("tool_use") if assistant => RecordKind::ToolUse(string(item, "id")?),
            Some("tool_result") if !assistant => RecordKind::ToolResult {
                id: string(item, "tool_use_id")?,
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

//! Pure per-operation data qualification. This supplies no OS/runtime authority.
use crate::task_consumer::{DecodeError, VersionLine, envelope, release};
use serde_json::Value;
mod protocol;
pub use protocol::{ConsumerState, RefreshWork};

pub struct Qualification<'a> {
    pub state_root: &'a str,
    pub os: &'a str,
    pub architecture: &'a str,
    pub version_line: VersionLine,
}
/// Non-Clone one-shot data permit: qualify a fresh version-info for every wait.
pub struct WatchPermit {
    line: VersionLine,
}
impl WatchPermit {
    pub fn qualify(bytes: &[u8], expected: Qualification<'_>) -> Result<Self, DecodeError> {
        if bytes.len() > 16384 {
            return Err(DecodeError::Size);
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| DecodeError::Json)?;
        validate_handshake(&raw, &expected)?;
        Ok(Self {
            line: expected.version_line,
        })
    }
    pub fn consume(self, bytes: &[u8]) -> Result<WatchDocument, DecodeError> {
        WatchDocument::parse(bytes, self.line)
    }
}
fn validate_handshake(raw: &Value, expected: &Qualification<'_>) -> Result<(), DecodeError> {
    if raw.get("format").and_then(Value::as_str) != Some("harflow.version-info")
        || raw.get("v").and_then(Value::as_u64) != Some(1)
        || raw.get("component").and_then(Value::as_str) != Some("engine")
        || raw.get("state_root").and_then(Value::as_str) != Some(expected.state_root)
        || raw.get("os").and_then(Value::as_str) != Some(expected.os)
        || raw.get("architecture").and_then(Value::as_str) != Some(expected.architecture)
        || raw.pointer("/contracts/ctl/format").and_then(Value::as_str) != Some("harflow.ctl")
        || raw.pointer("/contracts/ctl/v").and_then(Value::as_u64) != Some(1)
    {
        return Err(DecodeError::Envelope);
    }
    let mut ctl = raw.clone();
    ctl["format"] = Value::String("harflow.ctl".into());
    envelope(&ctl, expected.version_line)?;
    let caps = raw
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Field)?;
    if caps.iter().any(|v| !v.is_string())
        || ["ctl.v1", "version-info.v1", "task-list-watch.v1"]
            .iter()
            .any(|required| !caps.iter().any(|v| v.as_str() == Some(required)))
    {
        return Err(DecodeError::Field);
    }
    Ok(())
}
#[derive(Debug)]
pub struct WatchDocument {
    raw: Value,
    bytes: Vec<u8>,
}
impl WatchDocument {
    fn parse(bytes: &[u8], line: VersionLine) -> Result<Self, DecodeError> {
        if bytes.len() > 16384 {
            return Err(DecodeError::Size);
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| DecodeError::Json)?;
        envelope(&raw, line)?;
        match (raw.get("error"), raw.get("result")) {
            (Some(error), None)
                if error.is_object()
                    && error.get("code").is_some_and(Value::is_string)
                    && error.get("message").is_some_and(Value::is_string) => {}
            (None, Some(result)) => validate_result(result)?,
            _ => return Err(DecodeError::Payload),
        }
        Ok(Self {
            raw,
            bytes: bytes.into(),
        })
    }
    pub fn raw(&self) -> &Value {
        &self.raw
    }
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn state(&self) -> Option<&str> {
        self.raw.pointer("/result/state").and_then(Value::as_str)
    }
    pub fn cursor(&self) -> Option<&str> {
        self.raw.pointer("/result/cursor").and_then(Value::as_str)
    }
}
fn code(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn validate_result(result: &Value) -> Result<(), DecodeError> {
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .ok_or(DecodeError::Field)?;
    let cursor = result.get("cursor").ok_or(DecodeError::Field)?;
    if state == "unavailable" {
        if !cursor.is_null() {
            return Err(DecodeError::Field);
        }
    } else if !["baseline", "changed", "timeout", "reset"].contains(&state)
        || !cursor
            .as_str()
            .is_some_and(|c| !c.is_empty() && c.len() <= 2048)
    {
        return Err(DecodeError::Field);
    }
    if ["reset", "unavailable"].contains(&state) {
        if !result
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(code)
        {
            return Err(DecodeError::Field);
        }
    } else if result.get("reason").is_some() {
        return Err(DecodeError::Field);
    }
    Ok(())
}
/// Producer mapping takes an explicit stable version; it does not advertise support.
pub fn encode_result(
    result: &hf_domain::task_watch::WatchResult,
    version: &str,
) -> Result<Vec<u8>, DecodeError> {
    use hf_domain::task_watch::{WatchReason as R, WatchState as S};
    if release(version) != Some((0, 0)) {
        return Err(DecodeError::Version);
    }
    let state = match result.state {
        S::Baseline => "baseline",
        S::Changed => "changed",
        S::Timeout => "timeout",
        S::Reset => "reset",
        S::Unavailable => "unavailable",
    };
    let mut payload = serde_json::json!({"state":state,"cursor":result.cursor});
    if let Some(reason) = result.reason {
        payload["reason"] = Value::String(
            match reason {
                R::EpochChanged => "epoch_changed",
                R::Discontinuity => "discontinuity",
                R::StateMissing => "state_missing",
                R::ObservationUnavailable => "observation_unavailable",
            }
            .into(),
        );
    }
    validate_result(&payload)?;
    let bytes = serde_json::to_vec(&serde_json::json!({"format":"harflow.ctl","v":1,"product":"harflow","product_version":version,"result":payload})).map_err(|_| DecodeError::Json)?;
    if bytes.len() > 16384 {
        return Err(DecodeError::Size);
    }
    Ok(bytes)
}

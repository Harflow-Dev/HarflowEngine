//! Explicit stream-turn metadata, separate from existing closed fixture interpretation.
use hf_domain::backend::BackendError;
use serde_json::Value;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Boundary {
    None,
    Start,
    Stop,
    BlockStart(u64),
    BlockStop(u64),
    Delta(u64),
    MessageDelta,
}
pub(super) fn production_boundary(v: &Value) -> Result<Boundary, BackendError> {
    if v["type"] != "stream_event" {
        return Ok(Boundary::None);
    }
    if !v["parent_tool_use_id"].is_null() {
        return Err(BackendError::Unsupported);
    }
    let e = &v["event"];
    let index = || {
        e["index"]
            .as_u64()
            .filter(|i| *i < 128)
            .ok_or(BackendError::Shape)
    };
    match e["type"].as_str() {
        Some("message_start") => Ok(Boundary::Start),
        Some("message_stop") => Ok(Boundary::Stop),
        Some("content_block_start") => Ok(Boundary::BlockStart(index()?)),
        Some("content_block_stop") => Ok(Boundary::BlockStop(index()?)),
        Some("content_block_delta") => Ok(Boundary::Delta(index()?)),
        Some("message_delta") => Ok(Boundary::MessageDelta),
        _ => Err(BackendError::Unsupported),
    }
}
pub fn line_boundary(bytes: &[u8]) -> Result<Boundary, BackendError> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| BackendError::Shape)?;
    production_boundary(&v)
}

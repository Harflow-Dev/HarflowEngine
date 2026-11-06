//! Bounded scripted-provider transport. Parsed data never establishes authority.
use serde_json::{Value, json};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureCodecError {
    Budget,
    Shape,
    Nonce,
    Tools,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureRequest {
    pub model: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaudeEvent {
    Other,
    TextWitness,
    TerminalSuccess,
    TerminalFailure,
}
pub const MAX_BODY: usize = 24576;
pub const MAX_RESPONSE: usize = 8192;
fn nonce_valid(nonce: &str) -> bool {
    (16..=64).contains(&nonce.len()) && nonce.bytes().all(|b| b.is_ascii_alphanumeric())
}
/// Only a known one-use nonce in a user message permits the fixed no-tools reply.
pub fn decode_anthropic_request(
    raw: &[u8],
    nonce: &str,
) -> Result<FixtureRequest, FixtureCodecError> {
    if !nonce_valid(nonce) {
        return Err(FixtureCodecError::Nonce);
    }
    let value = bounded_json(raw)?;
    reject_tools(&value)?;
    let model = value["model"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(FixtureCodecError::Shape)?;
    let messages = value["messages"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 16)
        .ok_or(FixtureCodecError::Shape)?;
    if value
        .get("tools")
        .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
    {
        return Err(FixtureCodecError::Tools);
    }
    let expected = format!("Harflow fixture nonce {nonce}. Reply with HARFLOW_FIXTURE_OK:{nonce}.");
    let matched = messages
        .iter()
        .any(|m| m["role"].as_str() == Some("user") && content_matches(&m["content"], &expected));
    if !matched {
        return Err(FixtureCodecError::Nonce);
    }
    Ok(FixtureRequest {
        model: model.into(),
    })
}
fn content_matches(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
        || value.as_array().is_some_and(|a| {
            a.iter().any(|item| {
                item["type"].as_str() == Some("text") && item["text"].as_str() == Some(expected)
            })
        })
}
/// Fixed SSE body only. The HTTP owner must enforce route, nonce, bounds and settlement.
pub fn anthropic_text_response(model: &str, nonce: &str) -> Result<Vec<u8>, FixtureCodecError> {
    if !nonce_valid(nonce)
        || model.is_empty()
        || model.len() > 256
        || model.chars().any(char::is_control)
    {
        return Err(FixtureCodecError::Shape);
    }
    let events = [
        (
            "message_start",
            json!({"type":"message_start","message":{"id":format!("msg_{nonce}"),"type":"message","role":"assistant","content":[],"model":model,"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
        ),
        (
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":format!("HARFLOW_FIXTURE_OK:{nonce}")}}),
        ),
        (
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        (
            "message_delta",
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ];
    let mut body = Vec::new();
    for (kind, value) in events {
        body.extend_from_slice(format!("event: {kind}\ndata: {value}\n\n").as_bytes());
    }
    if body.len() > MAX_RESPONSE {
        return Err(FixtureCodecError::Budget);
    }
    Ok(body)
}
/// Terminal result is one protocol witness; callers still require PID exit and both EOFs.
pub fn decode_claude_event(raw: &[u8], nonce: &str) -> Result<ClaudeEvent, FixtureCodecError> {
    if !nonce_valid(nonce) {
        return Err(FixtureCodecError::Nonce);
    }
    let marker = format!("HARFLOW_FIXTURE_OK:{nonce}");
    let value = bounded_json(raw)?;
    reject_tools(&value)?;
    if !value.is_object() || !value["type"].is_string() {
        return Err(FixtureCodecError::Shape);
    }
    match value["type"].as_str() {
        Some("result") => {
            if value["subtype"].as_str() == Some("success")
                && value["is_error"].as_bool() == Some(false)
                && value["result"].as_str() == Some(marker.as_str())
            {
                Ok(ClaudeEvent::TerminalSuccess)
            } else {
                Ok(ClaudeEvent::TerminalFailure)
            }
        }
        Some("assistant") if content_matches(&value["message"]["content"], &marker) => {
            Ok(ClaudeEvent::TextWitness)
        }
        _ => Ok(ClaudeEvent::Other),
    }
}
fn bounded_json(raw: &[u8]) -> Result<Value, FixtureCodecError> {
    if raw.len() > MAX_BODY {
        return Err(FixtureCodecError::Budget);
    }
    crate::owner_profile::decode_for_fixture(raw).map_err(|_| FixtureCodecError::Shape)
}

fn reject_tools(value: &Value) -> Result<(), FixtureCodecError> {
    match value {
        Value::Object(fields) => {
            if fields
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.contains("tool") || kind == "input_json_delta")
                || fields.get("tools").is_some_and(|tools| {
                    !tools.is_null() && tools.as_array().is_none_or(|items| !items.is_empty())
                })
            {
                return Err(FixtureCodecError::Tools);
            }
            for child in fields.values() {
                reject_tools(child)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                reject_tools(child)?;
            }
        }
        _ => (),
    }
    Ok(())
}

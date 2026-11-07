//! The wire form of a page cursor: URL-safe base64 of compact JSON, opaque to clients.
//! `scope` binds it to the store generation and the filters that shaped the result.
use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use hf_domain::{ctl::task_rows::WireCursor, task_read::TaskId};
use serde_json::{Value, json};

const MAX_CHARS: usize = 2048;
const VERSION: i64 = 1;

pub fn encode(cursor: &WireCursor) -> String {
    let body = json!({"v": VERSION, "scope": cursor.scope, "stamp": cursor.stamp, "id": cursor.id});
    URL_SAFE_NO_PAD.encode(body.to_string())
}

/// Any malformed value, including one of another version, is `None`.
pub fn decode(raw: &str) -> Option<WireCursor> {
    if raw.is_empty() || raw.len() > MAX_CHARS {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| URL_SAFE.decode(raw))
        .ok()?;
    let Value::Object(map) = serde_json::from_slice(&bytes).ok()? else {
        return None;
    };
    if map.len() != 4 || map.get("v").and_then(Value::as_i64) != Some(VERSION) {
        return None;
    }
    let scope = map.get("scope")?.as_str()?;
    let id = map.get("id")?.as_str()?;
    TaskId::new(id.to_string()).ok()?;
    if scope.len() != 64 || !scope.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(WireCursor {
        scope: scope.to_string(),
        stamp: map.get("stamp")?.as_i64()?,
        id: id.to_string(),
    })
}

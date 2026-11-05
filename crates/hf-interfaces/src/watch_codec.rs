//! Bounded opaque cursor mapping after caller/root qualification by application ports.
use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use hf_application::task_watch::{WatchCodecPort, WatchError};
use hf_domain::task_watch::WatchCursor;
use serde_json::{Value, json};

pub struct WatchJsonCodec;
impl WatchCodecPort for WatchJsonCodec {
    fn decode(&self, raw: &str, expected_root: &str) -> Result<WatchCursor, WatchError> {
        if raw.is_empty() || raw.len() > 2048 {
            return Err(WatchError::InvalidCursor);
        }
        let bytes = URL_SAFE
            .decode(raw)
            .or_else(|_| URL_SAFE_NO_PAD.decode(raw))
            .map_err(|_| WatchError::InvalidCursor)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| WatchError::InvalidCursor)?;
        if value.get("v").and_then(Value::as_u64) != Some(1)
            || value.get("scope").and_then(Value::as_str) != Some("task-list-watch.v1")
        {
            return Err(WatchError::InvalidCursor);
        }
        let cursor = WatchCursor {
            root: value
                .get("root")
                .and_then(Value::as_str)
                .ok_or(WatchError::InvalidCursor)?
                .into(),
            epoch: value
                .get("epoch")
                .and_then(Value::as_str)
                .ok_or(WatchError::InvalidCursor)?
                .into(),
            revision: value
                .get("revision")
                .and_then(Value::as_i64)
                .ok_or(WatchError::InvalidCursor)?,
        };
        if !cursor.validate() || cursor.root != expected_root {
            return Err(WatchError::InvalidCursor);
        }
        Ok(cursor)
    }
    fn encode(&self, cursor: &WatchCursor) -> Result<String, WatchError> {
        if !cursor.validate() {
            return Err(WatchError::InvalidCursor);
        }
        let bytes = serde_json::to_vec(&json!({"v":1,"scope":"task-list-watch.v1",
            "root":cursor.root,"epoch":cursor.epoch,"revision":cursor.revision}))
        .map_err(|_| WatchError::InvalidCursor)?;
        Ok(URL_SAFE.encode(bytes))
    }
}

use super::{DecodeError, Value};
fn typed(raw: &Value, key: &str, check: fn(&Value) -> bool) -> Result<(), DecodeError> {
    if raw.get(key).is_some_and(|v| !v.is_null() && !check(v)) {
        return Err(DecodeError::Field);
    }
    Ok(())
}
fn unsigned(v: &Value) -> bool {
    v.as_u64().is_some()
}
fn integer(v: &Value) -> bool {
    v.as_i64().is_some()
}
pub(super) fn event_identity(raw: &Value) -> Result<(), DecodeError> {
    if !raw.is_object()
        || raw.get("product").and_then(Value::as_str) != Some("harflow")
        || raw.get("format").and_then(Value::as_str) != Some("harflow.event")
        || raw.get("v").and_then(Value::as_u64) != Some(1)
    {
        return Err(DecodeError::Envelope);
    }
    if raw
        .get("product_version")
        .and_then(Value::as_str)
        .and_then(crate::task_consumer::release)
        .is_none()
    {
        return Err(DecodeError::Version);
    }
    if raw.get("seq").and_then(Value::as_u64).is_none()
        || !raw.get("kind").is_some_and(Value::is_string)
    {
        return Err(DecodeError::Field);
    }
    Ok(())
}
pub(super) fn event(raw: &Value, bounded: bool) -> Result<(), DecodeError> {
    event_identity(raw)?;
    if bounded
        && serde_json::to_vec(raw)
            .map_err(|_| DecodeError::Json)?
            .len()
            > 16_384
    {
        return Err(DecodeError::Size);
    }
    for key in [
        "observed_at",
        "source_ts",
        "task_id",
        "text",
        "backend",
        "model",
        "status",
        "call_id",
        "name",
        "message",
    ] {
        typed(raw, key, Value::is_string)?;
    }
    for key in ["raw_offset", "input_tokens", "output_tokens", "bytes"] {
        typed(raw, key, unsigned)?;
    }
    typed(raw, "exit_code", integer)?;
    for key in ["is_error", "truncated", "oversized"] {
        typed(raw, key, Value::is_boolean)?;
    }
    Ok(())
}
pub(super) fn page(raw: &Value) -> Result<(), DecodeError> {
    let events = raw
        .get("events")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Payload)?;
    if events.len() > 200 {
        return Err(DecodeError::Size);
    }
    for item in events {
        event(item, true)?;
    }
    for key in ["task_id", "order", "change_token", "observed_at"] {
        typed(raw, key, Value::is_string)?;
    }
    for key in ["has_older", "has_newer", "has_more"] {
        typed(raw, key, Value::is_boolean)?;
    }
    for key in ["next_after_seq", "next_before_seq"] {
        typed(raw, key, unsigned)?;
    }
    for key in ["older_cursor", "newer_cursor", "next_cursor"] {
        typed(raw, key, Value::is_string)?;
        if raw
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| s.chars().count() > 2048)
        {
            return Err(DecodeError::Field);
        }
    }
    if let Some(kinds) = raw.get("kinds")
        && !(kinds.is_string()
            || kinds
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)))
    {
        return Err(DecodeError::Field);
    }
    object(raw, "span", &["first_seq", "last_seq"], &[], &[])?;
    object(raw, "skipped", &["malformed", "oversized"], &[], &[])?;
    object(
        raw,
        "log",
        &["final_seq"],
        &["state", "task_status", "capture_state"],
        &["output_drained"],
    )?;
    if let Some(gaps) = raw.get("gaps") {
        let gaps = gaps.as_array().ok_or(DecodeError::Field)?;
        if gaps.len() > 200 {
            return Err(DecodeError::Size);
        }
        for gap in gaps {
            if !gap.is_object() {
                return Err(DecodeError::Field);
            }
            for key in ["after_seq", "before_seq"] {
                typed(gap, key, unsigned)?;
            }
            typed(gap, "reason", Value::is_string)?;
        }
    }
    Ok(())
}
fn object(
    raw: &Value,
    key: &str,
    nums: &[&str],
    strings: &[&str],
    bools: &[&str],
) -> Result<(), DecodeError> {
    if let Some(value) = raw.get(key) {
        if value.is_null() {
            return Ok(());
        }
        if !value.is_object() {
            return Err(DecodeError::Field);
        }
        for k in nums {
            typed(value, k, unsigned)?;
        }
        for k in strings {
            typed(value, k, Value::is_string)?;
        }
        for k in bools {
            typed(value, k, Value::is_boolean)?;
        }
    }
    Ok(())
}

//! Canonical v1 recursive preview bounds and identity-preserving oversized stubs.
use hf_application::task_runtime::InvalidIntent;
use serde_json::{Value, json};
pub(super) fn bounded(mut value: Value) -> Result<(Vec<u8>, bool), InvalidIntent> {
    let object = value.as_object_mut().ok_or(InvalidIntent)?;
    let identity = object.clone();
    let mut changed = false;
    for (key, child) in object.iter_mut() {
        if !matches!(
            key.as_str(),
            "product"
                | "format"
                | "v"
                | "product_version"
                | "seq"
                | "kind"
                | "observed_at"
                | "source_ts"
                | "raw_offset"
                | "task_id"
        ) {
            changed |= truncate(child, 1);
        }
    }
    if changed {
        object.insert("truncated".into(), Value::Bool(true));
    }
    let body = serde_json::to_vec(&value).map_err(|_| InvalidIntent)?;
    if body.len() <= 16384 {
        return Ok((body, changed));
    }
    let mut stub = json!({"truncated":true,"oversized":true,"bytes":body.len()});
    let object = stub.as_object_mut().ok_or(InvalidIntent)?;
    for key in [
        "product",
        "format",
        "v",
        "product_version",
        "seq",
        "kind",
        "observed_at",
    ] {
        object.insert(key.into(), identity.get(key).cloned().ok_or(InvalidIntent)?);
    }
    Ok((serde_json::to_vec(&stub).map_err(|_| InvalidIntent)?, true))
}
fn truncate(value: &mut Value, depth: usize) -> bool {
    if depth > 4 {
        *value = Value::String("...".into());
        return true;
    }
    match value {
        Value::String(text) if text.chars().count() > 2000 => {
            *text = text.chars().take(2000).collect::<String>() + "...";
            true
        }
        Value::Array(items) => {
            let mut changed = items.len() > 50;
            items.truncate(50);
            for item in items {
                changed |= truncate(item, depth + 1);
            }
            changed
        }
        Value::Object(fields) => {
            let mut changed = false;
            for child in fields.values_mut() {
                changed |= truncate(child, depth + 1);
            }
            changed
        }
        _ => false,
    }
}

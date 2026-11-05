//! Strict, bounded classification of synthetic policy specimens; no execution authority.
mod decode;
mod fields;
mod models;
mod observation;
mod storage;
use fields::*;
use serde_json::{Value, json};
type Checked<T> = Result<T, (&'static str, &'static str)>;
fn invalid(reason: &'static str) -> (&'static str, &'static str) {
    ("invalid", reason)
}
fn unsupported(reason: &'static str) -> (&'static str, &'static str) {
    ("unsupported", reason)
}
pub fn classify(bytes: &[u8]) -> Value {
    match classify_checked(bytes) {
        Ok(value) => value,
        Err((parse, reason)) => json!({"parse":parse,"reason":reason}),
    }
}
fn classify_checked(bytes: &[u8]) -> Checked<Value> {
    let v = decode::decode(bytes)?;
    if !v.is_object()
        || v["format"].as_str() != Some("harflow.owner-runtime.profile")
        || v["v"].as_u64().is_none()
        || !v["kind"].is_string()
        || !v["binding"].is_object()
        || !v["data"].is_object()
    {
        return Err(invalid("envelope"));
    }
    let kind = text(&v, "kind");
    if v["v"].as_u64() != Some(1)
        || ![
            "policy",
            "permission",
            "observation",
            "storage",
            "steering_guard",
        ]
        .contains(&kind)
    {
        return Err(unsupported("envelope"));
    }
    binding(&v["binding"])?;
    let d = &v["data"];
    match kind {
        "policy" => models::policy(d),
        "permission" => models::permission(d),
        "observation" => observation::classify(d),
        "storage" => storage::classify(d, &v["binding"]),
        _ => models::steering(d),
    }
}
fn binding(v: &Value) -> Checked<()> {
    let root = &v["root"];
    let steering = &v["steering"];
    let base = [
        "operation",
        "session",
        "reservation",
        "attempt",
        "generation",
    ]
    .iter()
    .all(|k| token(&v[k], 32))
        && process(&v["owner"]).is_some()
        && root.is_object()
        && integer(&root["device"], 0, u64::MAX)
        && integer(&root["inode"], 1, u64::MAX)
        && integer(&root["protocol"], 0, u64::MAX)
        && ["request_digest", "configuration_digest"]
            .iter()
            .all(|k| token(&v[k], 64))
        && v.get("steering").is_some();
    let steering_valid = steering.is_null()
        || (steering.is_object()
            && steering["role"].is_string()
            && token(&steering["checkpoint"], 32)
            && integer(&steering["revision"], 0, u64::MAX)
            && token(&steering["issued_attempt"], 32)
            && steering["issued_attempt"] == v["attempt"]);
    if !base || !steering_valid {
        return Err(invalid("binding"));
    }
    if root["protocol"].as_u64() != Some(5)
        || (!steering.is_null()
            && !["worker", "builder", "orchestrator"].contains(&text(steering, "role")))
    {
        return Err(unsupported("binding"));
    }
    Ok(())
}

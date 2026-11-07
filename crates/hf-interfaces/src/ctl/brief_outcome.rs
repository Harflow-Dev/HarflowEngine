//! The bounded `outcome` and `failure_diagnostic` of a brief status.
use super::{bound::python_str, brief::text};
use serde_json::{Map, Value, json};

const OUTCOME: [&str; 8] = [
    "base_commit",
    "head_commit",
    "new_commits",
    "new_commits_truncated",
    "local_upstream",
    "pushed",
    "publication_evidence",
    "attribution",
];
const DIAGNOSTIC: [&str; 9] = [
    "category",
    "reason",
    "source",
    "reset_at",
    "background_idle_seconds",
    "elapsed_background_wait_seconds",
    "background_tasks",
    "latest_result",
    "successful_agent_result",
];
/// What reads add to an outcome's range semantics, as the reference words it.
const ATTRIBUTION: &str = "new_commits means newly reachable from the observed comparison base; imported history may be included. Task-created commits and authorship are unavailable.";
const VALUE_BYTES: usize = 160;
const KEY_BYTES: usize = 32;
const LIST_ITEMS: usize = 3;
const OBJECT_ITEMS: usize = 4;
const NESTING: usize = 2;
const ENDPOINT_BYTES: usize = 160;
const ENDPOINT_KEPT: usize = 146;

/// Scalars as they are, text cut, and at most two levels of three list items or four members.
fn compact(value: &Value, depth: usize) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(original) => json!(text(original, VALUE_BYTES)),
        Value::Array(items) if depth < NESTING => Value::Array(
            items
                .iter()
                .take(LIST_ITEMS)
                .map(|item| compact(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) if depth < NESTING => Value::Object(
            map.iter()
                .take(OBJECT_ITEMS)
                .map(|(key, item)| (text(key, KEY_BYTES), compact(item, depth + 1)))
                .collect(),
        ),
        _ => Value::Null,
    }
}

/// Bounded endpoint provenance: `head`, `branch` and `provenance` of each phase.
fn endpoints(outcome: &Map<String, Value>) -> Map<String, Value> {
    let bounded = |value: Option<&Value>| match value.and_then(Value::as_str) {
        Some(original) if original.len() <= ENDPOINT_BYTES => json!(original),
        Some(original) => json!(format!("{}...[truncated]", text(original, ENDPOINT_KEPT))),
        None => Value::Null,
    };
    let mut result = Map::new();
    for phase in ["baseline", "terminal"] {
        if let Some(Value::Object(value)) = outcome.get(phase) {
            let keys = ["head", "branch", "provenance"];
            let kept: Map<String, Value> = keys
                .iter()
                .map(|key| ((*key).to_string(), bounded(value.get(*key))))
                .collect();
            result.insert(phase.to_string(), Value::Object(kept));
        }
    }
    result
}

/// `outcome` (`key` true) or `failure_diagnostic`, projected into `result`; whether the
/// projection differs from the stored value.
pub(super) fn project(
    payload: &Map<String, Value>,
    outcome: bool,
    result: &mut Map<String, Value>,
) -> bool {
    let key = if outcome {
        "outcome"
    } else {
        "failure_diagnostic"
    };
    let Some(stored) = payload.get(key).filter(|value| !value.is_null()) else {
        return false;
    };
    let Value::Object(map) = stored else {
        result.insert(
            key.to_string(),
            json!(text(&python_str(stored), VALUE_BYTES)),
        );
        return true;
    };
    let mut value = map.clone();
    if outcome {
        value.insert("attribution".into(), json!(ATTRIBUTION));
    }
    let allowed: &[&str] = if outcome { &OUTCOME } else { &DIAGNOSTIC };
    let mut projected: Map<String, Value> = allowed
        .iter()
        .filter_map(|name| {
            value
                .get(*name)
                .map(|item| ((*name).to_string(), compact(item, 0)))
        })
        .collect();
    if outcome {
        projected.extend(endpoints(&value));
        if let Some(Value::Array(commits)) = value.get("new_commits") {
            let flagged = value.get("new_commits_truncated").is_some_and(truthy);
            projected.insert(
                "new_commits_truncated".into(),
                json!(flagged || commits.len() > LIST_ITEMS),
            );
        }
    }
    let cut = projected != value;
    result.insert(key.to_string(), Value::Object(projected));
    cut
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

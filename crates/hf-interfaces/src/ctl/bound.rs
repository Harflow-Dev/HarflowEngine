//! A listing header is at most 2 KiB; full content stays on the direct detail route. The
//! sizes are those of Python's `json.dumps(..., ensure_ascii=True)`, which the contract's
//! budget was written against.
use super::py_float::{Exact, Place, number_text, segment};
use serde_json::{Map, Value, json};

const HEADER_BYTES: usize = 2048;
const PROTECTED: [&str; 31] = [
    "task_id",
    "workflow_run_id",
    "parent_workflow_run_id",
    "root_workflow_run_id",
    "orchestrator_session_owner_run_id",
    "workflow_session_owner_run_id",
    "session_id",
    "parent_task_id",
    "continuation_of_task_id",
    "chain_root_task_id",
    "root_task_id",
    "spawned_by",
    "status",
    "started_at",
    "created_at",
    "updated_at",
    "backend",
    "kind",
    "execution_contract",
    "needs_direct_lookup",
    "sessions",
    "finished_at",
    "resumed_at",
    "workflow_id",
    "workflow_builder",
    "settling",
    "persisted_status",
    "observed_exit",
    "process_identity_state",
    "needs_reconciliation",
    "status_reconciled",
];
/// Fields never cut at all.
const METADATA: [&str; 9] = [
    "finished_at",
    "resumed_at",
    "workflow_id",
    "workflow_builder",
    "kind",
    "settling",
    "workflow_run_id",
    "parent_workflow_run_id",
    "root_workflow_run_id",
];

/// The serialized size of `value` as Python's `json.dumps` with `ensure_ascii` writes it.
pub fn python_size(value: &Value) -> usize {
    python_size_with(value, None)
}

/// As `python_size`, counting `exact` where it points.
pub fn python_size_with(value: &Value, exact: Option<Exact<'_>>) -> usize {
    sized(value, Style::ASCII, &mut Place::new(exact))
}

/// As `python_size` with `ensure_ascii=False`: non-ASCII text counts as UTF-8 bytes.
pub fn utf8_size(value: &Value) -> usize {
    sized(value, Style::UTF8, &mut Place::new(None))
}

/// As `python_size` with `separators=(",", ":")`, counting `exact` where it points.
pub fn compact_size_with(value: &Value, exact: Option<Exact<'_>>) -> usize {
    sized(value, Style::COMPACT, &mut Place::new(exact))
}

/// How `json.dumps` was asked to write: whether non-ASCII text is escaped, and the bytes of
/// the item and key separators.
#[derive(Clone, Copy)]
struct Style {
    ascii: bool,
    item: usize,
    key: usize,
}

impl Style {
    const ASCII: Self = Self {
        ascii: true,
        item: 2,
        key: 2,
    };
    const UTF8: Self = Self {
        ascii: false,
        item: 2,
        key: 2,
    };
    const COMPACT: Self = Self {
        ascii: true,
        item: 1,
        key: 1,
    };
}

fn sized(value: &Value, style: Style, at: &mut Place<'_>) -> usize {
    if let Some(digits) = at.exact_here() {
        return digits.len();
    }
    match value {
        Value::String(text) => string_size(text, style.ascii),
        Value::Array(items) => {
            let mut total = 2 + style.item * items.len().saturating_sub(1);
            for (index, item) in items.iter().enumerate() {
                let mark = at.enter(|| index.to_string());
                total += sized(item, style, at);
                at.leave(mark);
            }
            total
        }
        Value::Object(map) => {
            let mut total = 2 + style.item * map.len().saturating_sub(1);
            for (key, item) in map {
                let mark = at.enter(|| segment(key));
                total += string_size(key, style.ascii) + style.key + sized(item, style, at);
                at.leave(mark);
            }
            total
        }
        Value::Number(number) => number_text(number).len(),
        other => other.to_string().len(),
    }
}

fn string_size(text: &str, ascii: bool) -> usize {
    2 + text
        .chars()
        .map(|ch| match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            ' '..='~' => 1,
            // Control characters escape to \uXXXX; DEL and non-ASCII text escape only with
            // `ensure_ascii`, as a surrogate pair above the BMP.
            c if (c as u32) < 0x20 => 6,
            c if !ascii => c.len_utf8(),
            c if (c as u32) < 0x10000 => 6,
            _ => 12,
        })
        .sum::<usize>()
}

fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

fn too_big(map: &Map<String, Value>) -> bool {
    python_size(&Value::Object(map.clone())) > HEADER_BYTES
}

/// The bounded form of a merged task document. A header that cannot fit keeps only its
/// identity and a `needs_direct_lookup` marker; callers must then treat history as incomplete.
pub fn bound_header(header: &Map<String, Value>) -> Map<String, Value> {
    let mut result = header.clone();
    if let Some(Value::Object(sessions)) = result.get("sessions") {
        let orchestrator = sessions
            .get("orchestrator")
            .filter(|value| is_truthy(value));
        let normalized = match orchestrator {
            Some(value) => json!({"orchestrator": cut(&python_str(value), 128)}),
            None => json!({}),
        };
        result.insert("sessions".into(), normalized);
    }
    for (key, value) in result.iter_mut() {
        if let Value::String(text) = value
            && !METADATA.contains(&key.as_str())
        {
            let limit = if PROTECTED.contains(&key.as_str()) {
                128
            } else {
                200
            };
            *text = cut(text, limit);
        }
    }
    for maximum in [100, 32, 8] {
        if !too_big(&result) {
            return result;
        }
        let keys: Vec<String> = result.keys().cloned().collect();
        for key in keys {
            if PROTECTED.contains(&key.as_str()) {
                continue;
            }
            match result.get(&key) {
                Some(Value::String(text)) => {
                    let shorter = cut(text, maximum);
                    result.insert(key, Value::String(shorter));
                }
                Some(Value::Array(_) | Value::Object(_)) => {
                    result.remove(&key);
                }
                _ => {}
            }
        }
    }
    if too_big(&result) {
        result.retain(|key, _| PROTECTED.contains(&key.as_str()));
    }
    if too_big(&result) {
        let id = header.get("task_id").and_then(Value::as_str).unwrap_or("");
        let Value::Object(fallback) = json!({
            "task_id": cut(id, 100),
            "status": "unknown",
            "needs_direct_lookup": true,
            "index_blocker": "header_metadata_budget",
        }) else {
            return Map::new();
        };
        return fallback;
    }
    result
}

/// Whether the bounded header lost its content.
pub fn needs_direct_lookup(header: &Map<String, Value>) -> bool {
    header.get("needs_direct_lookup") == Some(&Value::Bool(true))
}

/// Python truthiness of a JSON value.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python's `str(value)` for scalars: text as it is, `True`, `False`, `None`, numbers. Lists
/// and objects keep their compact JSON form, where Python writes its own `repr`.
pub(super) fn python_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        other => other.to_string(),
    }
}

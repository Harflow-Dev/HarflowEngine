use super::{Value, validation};
use serde_json::{Map, json};
/// Classification of a complete fixture line, excluding its newline.
/// Storage owns byte spans and withholding partial tails.
#[derive(Debug, PartialEq)]
pub enum PreparedLine {
    Blank,
    Malformed,
    Oversized,
    Event { seq: u64, kind: String, body: Value },
}
pub fn prepare_line(raw: &[u8]) -> PreparedLine {
    if raw.is_empty() {
        return PreparedLine::Blank;
    }
    if raw.len() > 1_000_000 {
        return PreparedLine::Oversized;
    }
    let Ok(event) = serde_json::from_slice::<Value>(raw) else {
        return PreparedLine::Malformed;
    };
    // The raw log parser validates only identity and sequence/kind. Projection
    // sanitizes observed_at, rather than rejecting a historical observation.
    if validation::event_identity(&event).is_err() {
        return PreparedLine::Malformed;
    }
    let Some(seq) = event
        .get("seq")
        .and_then(Value::as_u64)
        .filter(|n| *n <= i64::MAX as u64)
    else {
        return PreparedLine::Malformed;
    };
    let Some(kind) = event.get("kind").and_then(Value::as_str) else {
        return PreparedLine::Malformed;
    };
    // Fixture storage has an explicit bounded kind index; refusal is preferable
    // to mismatching the metadata and the projected body.
    if kind.len() > 128 {
        return PreparedLine::Malformed;
    }
    let body = project(&event, seq, kind);
    PreparedLine::Event {
        seq,
        kind: kind.to_owned(),
        body,
    }
}
fn project(event: &Value, seq: u64, kind: &str) -> Value {
    let mut fields = Map::new();
    if let Some(map) = event.as_object() {
        for (key, value) in map {
            if ![
                "product",
                "format",
                "v",
                "product_version",
                "seq",
                "kind",
                "observed_at",
                "source_ts",
                "raw_offset",
                "task_id",
            ]
            .contains(&key.as_str())
            {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    let (mut body, changed) = truncate(Value::Object(fields), 0);
    let observed = event
        .get("observed_at")
        .and_then(Value::as_str)
        .filter(|s| s.chars().count() <= 128);
    if let Some(map) = body.as_object_mut() {
        map.insert("seq".into(), json!(seq));
        map.insert("kind".into(), json!(kind));
        map.insert("observed_at".into(), json!(observed));
        if changed {
            map.insert("truncated".into(), Value::Bool(true));
        }
        for key in ["product", "format", "v", "product_version"] {
            map.insert(key.into(), event[key].clone());
        }
    }
    let size = python_size(&body);
    if size > 16_384 {
        return json!({"product":event["product"],"format":event["format"],"v":event["v"],"product_version":event["product_version"],"seq":seq,"kind":kind,"observed_at":observed,"truncated":true,"oversized":true,"bytes":size});
    }
    body
}
// Python json.dumps adds a space after each object colon and each collection
// separator. Count these outside strings to reproduce its UTF-8 budget.
fn python_size(value: &Value) -> usize {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return usize::MAX;
    };
    let mut quoted = false;
    let mut escaped = false;
    let mut separators = 0;
    for byte in &bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else if *byte == b'"' {
            quoted = true;
        } else if *byte == b':' || *byte == b',' {
            separators += 1;
        }
    }
    bytes.len() + separators
}
fn truncate(value: Value, depth: usize) -> (Value, bool) {
    if depth > 4 {
        return (json!("..."), true);
    }
    match value {
        Value::String(s) if s.chars().count() > 2000 => (
            json!(s.chars().take(2000).collect::<String>() + "..."),
            true,
        ),
        Value::Array(items) => {
            let mut changed = items.len() > 50;
            let result = items
                .into_iter()
                .take(50)
                .map(|v| {
                    let (v, c) = truncate(v, depth + 1);
                    changed |= c;
                    v
                })
                .collect();
            (Value::Array(result), changed)
        }
        Value::Object(items) => {
            let mut changed = false;
            let result = items
                .into_iter()
                .map(|(k, v)| {
                    let (v, c) = truncate(v, depth + 1);
                    changed |= c;
                    (k, v)
                })
                .collect();
            (Value::Object(result), changed)
        }
        other => (other, false),
    }
}

//! One task as a wire document: the served header and status bytes, the typed facts and
//! the live projection merged into a single object. Keys with no stored source are absent.
use hf_application::ctl::task_reads::TaskView;
use hf_domain::ctl::{
    task_projection::{Projection, is_unobserved},
    task_rows::TaskFacts,
};
use serde_json::{Map, Value, json};

/// A stored blob that cannot be parsed, or that disagrees with the typed facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inconsistent;

fn object(bytes: &[u8]) -> Result<Map<String, Value>, Inconsistent> {
    match serde_json::from_slice(bytes) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(Inconsistent),
    }
}

fn text(value: &Option<String>) -> Value {
    value.as_ref().map_or(Value::Null, |text| json!(text))
}

/// Typed facts and the blob keys that must equal them when present.
fn facts_keys(facts: &TaskFacts) -> [(&'static str, Value); 12] {
    let link = |id: &Option<hf_domain::task_read::TaskId>| {
        id.as_ref().map_or(Value::Null, |id| json!(id.as_str()))
    };
    [
        ("task_id", json!(facts.id.as_str())),
        ("backend", json!(facts.backend)),
        ("repo_path", json!(facts.repo_path)),
        ("session_id", json!(facts.session_id)),
        ("started_at", json!(facts.started_at)),
        ("title", text(&facts.title)),
        ("model", text(&facts.model)),
        ("continuation_of_task_id", link(&facts.continuation_of)),
        (
            "chain_root_task_id",
            facts.chain_root.raw().map_or(Value::Null, |raw| json!(raw)),
        ),
        (
            "exit_code",
            facts.exit_code.map_or(Value::Null, |code| json!(code)),
        ),
        ("finished_at", text(&facts.finished_at)),
        ("output_drained", json!(facts.output_drained)),
    ]
}

fn agree(
    blob: &Map<String, Value>,
    facts: &TaskFacts,
    projection: &Projection,
) -> Result<(), Inconsistent> {
    for (key, expected) in facts_keys(facts) {
        if blob.get(key).is_some_and(|stored| *stored != expected) {
            return Err(Inconsistent);
        }
    }
    if blob
        .get("status")
        .is_some_and(|stored| *stored != json!(facts.status))
    {
        return Err(Inconsistent);
    }
    // The observation fields are the classifier's rule, not stored facts: a blob that holds
    // another value for either one disagrees.
    let same = |key: &str, expected: Value| blob.get(key).is_none_or(|stored| *stored == expected);
    if same("recovered", json!(projection.recovered))
        && same("observation_mode", json!(projection.observation_mode))
    {
        Ok(())
    } else {
        Err(Inconsistent)
    }
}

/// Seconds between start and finish, rounded to milliseconds. While unfinished the end is
/// the clock; with no clock the duration is unknown.
fn duration(facts: &TaskFacts, now_us: Option<i64>) -> Value {
    let end = facts.finished_at_us.or(now_us);
    match end.and_then(|end| end.checked_sub(facts.started_at_us)) {
        Some(micros) => json!((micros as f64 / 1000.0).round() / 1000.0),
        None => Value::Null,
    }
}

/// The document of one task. Status, activity and reconciliation always come from the
/// live projection, never from a recorded value.
pub fn task_document(
    view: &TaskView,
    now_us: Option<i64>,
) -> Result<Map<String, Value>, Inconsistent> {
    let facts = &view.record.facts;
    let projection = &view.projection;
    let header = object(&view.record.header)?;
    agree(&header, facts, projection)?;
    let mut document = header;
    if let Some(bytes) = &view.record.status_blob {
        let status = object(bytes)?;
        agree(&status, facts, projection)?;
        document.extend(status);
    }
    for (key, value) in facts_keys(facts) {
        document.insert(key.to_string(), value);
    }
    // Activity is the classifier's internal value; no document carries it.
    document.remove("active");
    document.insert("persisted_status".into(), json!(facts.status));
    document.insert(
        "observed_exit".into(),
        json!(!is_unobserved(&facts.status, facts.exit_code)),
    );
    document.insert("duration_seconds".into(), duration(facts, now_us));
    document.insert("recovered".into(), json!(projection.recovered));
    document.insert(
        "observation_mode".into(),
        json!(projection.observation_mode),
    );
    match projection.note {
        Some(note) => document.insert("note".into(), json!(note)),
        // A stored note is the classifier's to give, never a recorded value.
        None => document.remove("note"),
    };
    document.insert("status".into(), json!(projection.status));
    document.insert(
        "process_identity_state".into(),
        json!(projection.identity_state),
    );
    document.insert(
        "needs_reconciliation".into(),
        json!(projection.needs_reconciliation),
    );
    Ok(document)
}

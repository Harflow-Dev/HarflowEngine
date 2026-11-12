//! Results of `task-pending-page` and `task-pending-status`.
use super::{bound::python_size, history_docs::iso_micros, pending_cursor};
use hf_application::ctl::task_reads::{PendingItem, PendingPageOut, PendingStatusOut};
use hf_domain::ctl::pending::{LaunchOutcome, LaunchView, SCOPE};
use serde_json::{Map, Value, json};

fn launch(view: &LaunchView) -> Value {
    let lineage: Map<String, Value> = view
        .lineage
        .iter()
        .map(|(key, value)| ((*key).to_owned(), json!(value)))
        .collect();
    json!({
        "title": view.title,
        "backend": view.backend,
        "repo_path": view.repo_path,
        "created_at": view.created_at,
        "not_started_reason": view.not_started_reason,
        "generation": view.generation,
        "state": view.state,
        "lineage": lineage,
        "resource_names": view.resource_names,
        "metadata_truncated": view.metadata_truncated,
    })
}

fn support(view: &LaunchView) -> &'static str {
    if view.cancel_support {
        "supported"
    } else {
        "unsupported"
    }
}

fn item(item: &PendingItem) -> Value {
    match item {
        PendingItem::Pending { id, view } => json!({
            "task_id": id,
            "disposition": "pending",
            "launch": launch(view),
            "cancel_support": support(view),
        }),
        PendingItem::Refused { id, code, check } => json!({
            "task_id": id,
            "disposition": "refused",
            "code": code,
            "check": check,
        }),
    }
}

/// The serialized size of one listed item, for the response bound.
pub(super) fn item_size(listed: &PendingItem) -> usize {
    python_size(&item(listed))
}

fn observed(now: Option<i64>) -> Value {
    now.map_or(Value::Null, |micros| json!(iso_micros(micros)))
}

pub(super) fn page_result(out: &PendingPageOut, now: Option<i64>) -> Value {
    let coverage = if out.coverage_complete {
        "complete"
    } else {
        "unknown"
    };
    json!({
        "scope": SCOPE,
        "observed_at": observed(now),
        "items": out.items.iter().map(item).collect::<Vec<_>>(),
        "namespace_revision": out.namespace_revision,
        "namespace_complete": true,
        "coverage": coverage,
        "invalid_name_count": out.invalid_name_count,
        "examined_count": out.examined_count,
        "has_more": out.has_more,
        "next_cursor": out.next.as_ref().map(pending_cursor::encode),
        "bound": out.bound,
    })
}

pub(super) fn status_result(out: &PendingStatusOut, now: Option<i64>) -> Value {
    let mut result = json!({
        "scope": SCOPE,
        "observed_at": observed(now),
        "task_id": out.id,
    });
    let extra = match &out.outcome {
        None => json!({"disposition": "not_found"}),
        Some(LaunchOutcome::Recorded) => json!({"disposition": "recorded"}),
        Some(LaunchOutcome::Pending(view)) => json!({
            "disposition": "pending",
            "launch": launch(view),
            "cancel_support": support(view),
        }),
        Some(LaunchOutcome::Refused { code, check }) => {
            json!({"disposition": "refused", "code": code, "check": check})
        }
        Some(LaunchOutcome::Inconsistent) => json!({
            "disposition": "unknown",
            "code": "pending_record_inconsistent",
            "check": "postrecord_source_without_record",
        }),
    };
    if let (Some(map), Value::Object(more)) = (result.as_object_mut(), extra) {
        map.extend(more);
    }
    result
}

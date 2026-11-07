//! Success documents of the task reads.
use super::{
    bound::{bound_header, needs_direct_lookup, python_size},
    cursor,
    task_docs::{Inconsistent, task_document},
};
use hf_application::ctl::task_reads::{BatchOut, PageOut, StatusOut, TaskView};
use hf_domain::{process::ProcessIdentity, timestamp::civil_from_days};
use serde_json::{Map, Value, json};

use super::read_errors::{ReadPath, describe};

const OBSERVER_NOTE: &str = "Parent-process identity could not be qualified alive; observer continuity cannot be determined.";

/// Process-start evidence as a UTC instant.
fn start_time(identity: &ProcessIdentity) -> String {
    let seconds = i64::try_from(identity.start_seconds()).unwrap_or(0);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}+00:00",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        identity.start_microseconds()
    )
}

/// `--observer-parent`: the qualified parent, once per invocation, on every task.
fn observe(document: &mut Map<String, Value>, parent: &Option<Option<ProcessIdentity>>) {
    let Some(parent) = parent else { return };
    document.insert("observer_kind".into(), json!("parent_process"));
    match parent {
        Some(identity) => {
            document.insert(
                "observing_process".into(),
                json!({"pid": identity.pid(), "start_time": start_time(identity)}),
            );
            document.insert("observer_state".into(), json!("alive"));
        }
        None => {
            document.insert("observing_process".into(), Value::Null);
            document.insert("observer_state".into(), json!("undecidable"));
            document.insert("observation_note".into(), json!(OBSERVER_NOTE));
        }
    }
}

fn prefix_echo(document: &mut Map<String, Value>, from: &Option<String>) {
    if let Some(input) = from {
        let id = document.get("task_id").cloned().unwrap_or(Value::Null);
        document.insert(
            "resolved_from".into(),
            json!({"input": input, "task_id": id}),
        );
    }
}

fn documents(views: &[TaskView], now: Option<i64>) -> Result<Vec<Value>, Inconsistent> {
    views
        .iter()
        .map(|view| task_document(view, now).map(Value::Object))
        .collect()
}

/// A page header: the merged document held to the 2 KiB listing bound.
fn bounded(view: &TaskView, now: Option<i64>) -> Result<Map<String, Value>, Inconsistent> {
    task_document(view, now).map(|document| bound_header(&document))
}

/// The serialized size of a page item, for the page's byte budget. An inconsistent item
/// counts as nothing; rendering the page then fails with it.
pub fn item_size(view: &TaskView, now: Option<i64>) -> usize {
    bounded(view, now).map_or(0, |map| python_size(&Value::Object(map)))
}

pub fn list_body(views: &[TaskView], now: Option<i64>) -> Result<Value, Inconsistent> {
    Ok(json!({
        "tasks": documents(views, now)?,
        "history_incomplete": false,
        "refused_tasks": [],
    }))
}

pub fn status_task(
    out: &StatusOut,
    now: Option<i64>,
    parent: &Option<Option<ProcessIdentity>>,
) -> Result<Value, Inconsistent> {
    let mut document = task_document(&out.view, now)?;
    observe(&mut document, parent);
    prefix_echo(&mut document, &out.resolved_from);
    Ok(Value::Object(document))
}

/// `state_root` names the root in a batch entry's state error.
pub fn batch_body(
    out: &BatchOut,
    now: Option<i64>,
    parent: &Option<Option<ProcessIdentity>>,
    state_root: &str,
) -> Result<Value, Inconsistent> {
    let mut tasks = Vec::new();
    for row in &out.rows {
        let mut document = task_document(&row.view, now)?;
        observe(&mut document, parent);
        prefix_echo(&mut document, &row.resolved_from);
        tasks.push(Value::Object(document));
    }
    let mut errors = Map::new();
    for (raw, error) in &out.errors {
        errors.insert(
            raw.clone(),
            describe(error, ReadPath::Managed, state_root).object(),
        );
    }
    Ok(json!({"tasks": tasks, "errors": errors}))
}

fn catalog_state() -> Value {
    json!({
        "status": "ready",
        "source": "task_catalog",
        "pending_records": 0,
        "blocked_records": 0,
    })
}

fn withheld_body(reason: &str) -> Value {
    json!({
        "items": [],
        "related_headers": [],
        "next_cursor": null,
        "has_more": false,
        "bootstrap_pending": false,
        "history_incomplete": true,
        "authority_incomplete": true,
        "counts_complete": false,
        "catalog_state": {"status": "blocked", "source": "task_catalog", "reason": reason},
        "note": reason,
    })
}

pub fn page_body(page: &PageOut, now: Option<i64>) -> Result<Value, Inconsistent> {
    if let Some(reason) = &page.withheld {
        return Ok(withheld_body(reason));
    }
    let items = page
        .items
        .iter()
        .map(|view| bounded(view, now))
        .collect::<Result<Vec<_>, _>>()?;
    let related = page
        .related
        .iter()
        .map(|view| bounded(view, now))
        .collect::<Result<Vec<_>, _>>()?;
    // A header that could not be bounded says nothing complete about history or counts.
    let lost = items.iter().chain(&related).any(needs_direct_lookup);
    let rows =
        |maps: Vec<Map<String, Value>>| maps.into_iter().map(Value::Object).collect::<Vec<_>>();
    let mut body = Map::new();
    body.insert("items".into(), json!(rows(items)));
    body.insert("related_headers".into(), json!(rows(related)));
    body.insert(
        "next_cursor".into(),
        page.next
            .as_ref()
            .map_or(Value::Null, |next| json!(cursor::encode(next))),
    );
    body.insert("has_more".into(), json!(page.has_more));
    body.insert("bootstrap_pending".into(), json!(false));
    if lost {
        body.insert("history_incomplete".into(), json!(true));
        body.insert("counts_complete".into(), json!(false));
        for key in [
            "total_active_count",
            "total_active_root_count",
            "total_attention_root_count",
        ] {
            body.insert(key.into(), Value::Null);
        }
    }
    if let Some(counts) = &page.counts {
        let known = counts.complete && !lost;
        body.insert("history_incomplete".into(), json!(lost));
        body.insert("catalog_state".into(), catalog_state());
        body.insert("counts_complete".into(), json!(known));
        let count = |value: Option<u64>| if lost { None } else { value };
        body.insert("total_active_count".into(), json!(count(counts.active)));
        body.insert(
            "total_active_root_count".into(),
            json!(count(counts.active_root)),
        );
        body.insert(
            "total_attention_root_count".into(),
            json!(count(counts.attention_root)),
        );
    }
    if !page.resolved_task_ids.is_empty() {
        let echoes: Vec<Value> = page
            .resolved_task_ids
            .iter()
            .map(|(input, id)| json!({"input": input, "task_id": id}))
            .collect();
        body.insert("resolved_from_task_ids".into(), json!(echoes));
    }
    if let Some((id, from)) = &page.predecessor {
        body.insert("predecessor_task_id".into(), json!(id));
        body.insert("successor_scope".into(), json!("recorded_tasks"));
        if let Some(input) = from {
            body.insert(
                "resolved_from_predecessor_task_id".into(),
                json!({"input": input, "task_id": id}),
            );
        }
    }
    Ok(Value::Object(body))
}

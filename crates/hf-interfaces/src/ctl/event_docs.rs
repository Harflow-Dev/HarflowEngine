//! The `task-events` result: events rendered from their stored bodies, cursors, span,
//! gaps and the log object. A stored body is never served as it is.
use super::{
    bound::{python_size, utf8_size},
    event_cursor,
    history_docs::iso_micros,
    task_docs::Inconsistent,
};
use hf_application::{ctl::task_reads::EventsOut, event_read::EventRecord};
use hf_domain::{
    ctl::identity::{CTL_FORMAT, EVENT_FORMAT, PRODUCT, PRODUCT_VERSION, WIRE_VERSION},
    event_read::{EventFilter, EventGap, GapReason},
};
use serde_json::{Map, Value, json};

/// One rendered event is at most this many bytes under `ensure_ascii=False` sizing.
const EVENT_BYTES: usize = 16_384;
const STRING_CHARS: usize = 2000;
const LIST_ITEMS: usize = 50;
const DEPTH: usize = 4;
/// The whole printed document, less a reserve for the prefix echo added after measuring.
pub const DOCUMENT_BYTES: usize = 262_144;
const RESERVE_BYTES: usize = 512;
/// Fields of the envelope and of the stored line that are not event content.
const NOT_CONTENT: [&str; 10] = [
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
];

/// Strings, lists and nesting cut to their bounds; whether anything was cut.
fn truncate(value: Value, depth: usize) -> (Value, bool) {
    if depth > DEPTH {
        return (json!("..."), true);
    }
    match value {
        Value::String(text) if text.chars().count() > STRING_CHARS => {
            let mut cut: String = text.chars().take(STRING_CHARS).collect();
            cut.push_str("...");
            (Value::String(cut), true)
        }
        Value::Array(items) => {
            let mut changed = items.len() > LIST_ITEMS;
            let kept = items.into_iter().take(LIST_ITEMS).map(|item| {
                let (item, cut) = truncate(item, depth + 1);
                changed |= cut;
                item
            });
            (Value::Array(kept.collect()), changed)
        }
        Value::Object(map) => {
            let mut changed = false;
            let kept: Map<String, Value> = map
                .into_iter()
                .map(|(key, item)| {
                    let (item, cut) = truncate(item, depth + 1);
                    changed |= cut;
                    (key, item)
                })
                .collect();
            (Value::Object(kept), changed)
        }
        other => (other, false),
    }
}

fn stamp(mut event: Map<String, Value>) -> Map<String, Value> {
    event.insert("product".into(), json!(PRODUCT));
    event.insert("format".into(), json!(EVENT_FORMAT));
    event.insert("v".into(), json!(WIRE_VERSION));
    event.insert("product_version".into(), json!(PRODUCT_VERSION));
    event
}

/// The event of a stored body, with this engine's envelope and the bounds of the contract.
fn render(record: &EventRecord) -> Result<Value, Inconsistent> {
    let (Some(seq), Some(kind)) = (record.seq, record.kind.as_ref()) else {
        return Err(Inconsistent);
    };
    let Ok(Value::Object(body)) = serde_json::from_slice(&record.body) else {
        return Err(Inconsistent);
    };
    let observed = body
        .get("observed_at")
        .and_then(Value::as_str)
        .filter(|text| text.chars().count() <= 128)
        .map_or(Value::Null, |text| json!(text));
    let content: Map<String, Value> = body
        .into_iter()
        .filter(|(key, _)| !NOT_CONTENT.contains(&key.as_str()))
        .collect();
    let (content, cut) = truncate(Value::Object(content), 0);
    let Value::Object(mut event) = content else {
        return Err(Inconsistent);
    };
    event.insert("seq".into(), json!(seq));
    event.insert("kind".into(), json!(kind));
    event.insert("observed_at".into(), observed.clone());
    if cut {
        event.insert("truncated".into(), json!(true));
    }
    let event = stamp(event);
    let size = utf8_size(&Value::Object(event.clone()));
    if size <= EVENT_BYTES {
        return Ok(Value::Object(event));
    }
    let mut stub = Map::new();
    stub.insert("seq".into(), json!(seq));
    stub.insert("kind".into(), json!(kind));
    stub.insert("observed_at".into(), observed);
    stub.insert("truncated".into(), json!(true));
    stub.insert("oversized".into(), json!(true));
    stub.insert("bytes".into(), json!(size));
    Ok(Value::Object(stamp(stub)))
}

/// `"default"`, `"all"` or the sorted explicit list.
fn kinds_label(filter: &EventFilter) -> Value {
    match filter {
        EventFilter::Default => json!("default"),
        EventFilter::All => json!("all"),
        EventFilter::Kinds(_) => json!(filter.kinds()),
    }
}

fn gap(gap: &EventGap) -> Value {
    let reason = match gap.reason {
        GapReason::Unknown => "unknown",
        GapReason::UnreadableRecords => "unreadable_records",
    };
    json!({"after_seq": gap.after_seq, "before_seq": gap.before_seq, "reason": reason})
}

/// The `result` object. `now` stamps `observed_at`.
pub fn event_result(
    out: &EventsOut,
    filter: &EventFilter,
    now: Option<i64>,
) -> Result<Value, Inconsistent> {
    let page = &out.page;
    let events = page
        .events
        .iter()
        .map(render)
        .collect::<Result<Vec<_>, _>>()?;
    let log = &out.log;
    let mut result = json!({
        "task_id": out.task.as_str(),
        "order": "ascending",
        "kinds": kinds_label(filter),
        "events": events,
        "older_cursor": page.older_cursor.as_ref().map(event_cursor::encode),
        "newer_cursor": page.newer_cursor.as_ref().map(event_cursor::encode),
        "has_older": page.has_older,
        "has_newer": page.has_newer,
        "span": page.span.map(|(first, last)| json!({"first_seq": first, "last_seq": last})),
        "skipped": {"malformed": page.malformed, "oversized": page.oversized},
        "gaps": page.gaps.iter().map(gap).collect::<Vec<_>>(),
        "log": {
            "state": log.state.as_str(),
            "task_status": log.task_status,
            "output_drained": log.output_drained,
            "capture_state": log.capture.as_str(),
            "final_seq": log.final_seq,
        },
        "change_token": out.change_token,
        "observed_at": now.map(iso_micros),
    });
    if let (Some(input), Some(map)) = (&out.resolved_from, result.as_object_mut()) {
        map.insert(
            "resolved_from".into(),
            json!({"input": input, "task_id": out.task.as_str()}),
        );
    }
    Ok(result)
}

/// Whether the whole printed document fits its bound: sized as Python's `ensure_ascii`
/// output, newline included, less the reserve for the prefix echo. A page that cannot be
/// rendered counts as fitting, so the failure surfaces from the final render.
pub fn fits(out: &EventsOut, filter: &EventFilter, now: Option<i64>) -> bool {
    let Ok(result) = event_result(out, filter, now) else {
        return true;
    };
    let document = json!({
        "format": CTL_FORMAT,
        "product": PRODUCT,
        "product_version": PRODUCT_VERSION,
        "result": result,
        "v": WIRE_VERSION,
    });
    python_size(&document) + 1 + RESERVE_BYTES <= DOCUMENT_BYTES
}

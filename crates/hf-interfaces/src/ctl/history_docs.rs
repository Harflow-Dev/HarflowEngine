//! Results of `task-prompt` and `task-artifacts`, the artifact summary and the artifact
//! cursor.
use super::{bound::python_size, py_json::parse, py_value::Json};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use hf_application::ctl::task_reads::{
    ArtifactCursorArg, ArtifactEntry, ArtifactPage, NextCursor, PromptOut,
};
use hf_domain::{
    ctl::history::{ArtifactItem, ArtifactSummary},
    timestamp::civil_from_days,
};
use serde_json::{Value, json};

const CURSOR_CHARS: usize = 512;

/// A UTC instant as Python's `isoformat()` writes it: the fraction only when not zero.
pub fn iso_micros(micros: i64) -> String {
    let seconds = micros.div_euclid(1_000_000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    let fraction = micros.rem_euclid(1_000_000);
    let tail = if fraction == 0 {
        String::new()
    } else {
        format!(".{fraction:06}")
    };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{tail}+00:00",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

fn echo(result: &mut Value, task: &str, from: &Option<String>) {
    if let (Some(input), Some(map)) = (from, result.as_object_mut()) {
        map.insert(
            "resolved_from".into(),
            json!({"input": input, "task_id": task}),
        );
    }
}

pub fn prompt_result(out: &PromptOut) -> Value {
    let view = &out.view;
    let mut result = json!({
        "task_id": out.task.as_str(),
        "title": out.title,
        "prompt": {
            "state": view.state,
            "text": view.text,
            "returned_bytes": view.returned_bytes,
            "total_bytes": view.total_bytes,
            "truncated": view.truncated,
            "complete": view.complete,
            "integrity": view.integrity,
        },
    });
    echo(&mut result, out.task.as_str(), &out.resolved_from);
    result
}

/// `artifact_summary` of a task document.
pub fn summary_json(summary: &ArtifactSummary, task: &str) -> Value {
    let state = if summary.available {
        "available"
    } else {
        "unavailable"
    };
    let mut value = json!({"count": summary.count, "state": state});
    if let (true, Some(map)) = (summary.detail_ref, value.as_object_mut()) {
        map.insert(
            "detail_ref".into(),
            json!({"tool": "get_task_artifacts", "task_id": task}),
        );
    }
    value
}

/// `provenance` of an item: the method and task, plus the tool-use id and recorded plan
/// directory of an observed backend plan.
fn provenance(item: &ArtifactItem, task: &str) -> Value {
    let mut value = json!({"method": item.method, "task_id": task});
    let (Some(map), Some(plan)) = (value.as_object_mut(), &item.plan) else {
        return value;
    };
    map.insert("tool_use_id".into(), json!(plan.tool_use_id));
    if let Some(directory) = &plan.directory {
        map.insert("plan_directory".into(), json!(directory.path));
        map.insert(
            "plan_directory_identity".into(),
            json!({"device": directory.device, "inode": directory.inode}),
        );
    }
    value
}

fn entry_json(entry: &ArtifactEntry, task: &str, observed_at: &Value) -> Value {
    let item = &entry.item;
    let seen = item.registered;
    json!({
        "artifact_id": item.artifact_id,
        "task_id": task,
        "path": item.path,
        "kind": item.kind,
        "note": item.note,
        "registered_at": item.registered_at,
        "provenance": provenance(item, task),
        "registered_observation": {
            "device": seen.device,
            "inode": seen.inode,
            "size": seen.size,
            "mtime_ns": seen.mtime_ns,
        },
        "availability": {"state": entry.availability.as_str(), "observed_at": observed_at},
    })
}

/// The page as a candidate with these entries, as the reference measures it: the resolved
/// task, no cursor, `has_more` false, count 0 and no prefix echo. With no clock the stamp
/// is a full-width one, so the bound is never undercounted.
pub fn artifact_size(
    task: &str,
    readable: bool,
    entries: &[ArtifactEntry],
    now: Option<i64>,
) -> usize {
    let observed = json!(iso_micros(now.unwrap_or(1)));
    let items: Vec<Value> = entries
        .iter()
        .map(|entry| entry_json(entry, task, &observed))
        .collect();
    let state = if readable { "available" } else { "unavailable" };
    python_size(&json!({
        "task_id": task,
        "items": items,
        "next_cursor": null,
        "has_more": false,
        "state": state,
        "count": 0,
    }))
}

pub fn artifact_result(page: &ArtifactPage, now: Option<i64>) -> Value {
    let task = page.task.as_str();
    let observed = now.map_or(Value::Null, |micros| json!(iso_micros(micros)));
    let items: Vec<Value> = page
        .entries
        .iter()
        .map(|entry| entry_json(entry, task, &observed))
        .collect();
    let mut result = json!({
        "task_id": task,
        "items": items,
        "next_cursor": page.next.as_ref().map(encode_cursor),
        "has_more": page.has_more,
        "state": if page.readable { "available" } else { "unavailable" },
        "count": page.count,
    });
    echo(&mut result, task, &page.resolved_from);
    result
}

/// `base64.urlsafe_b64encode(json.dumps([task, revision, offset]))`: the same bytes the
/// reference writes, so a cursor decodes to `["task", "revision", 2]` with Python's spacing.
fn encode_cursor(next: &NextCursor) -> String {
    let text = |value: &str| json!(value).to_string();
    let body = format!(
        "[{}, {}, {}]",
        text(&next.task),
        text(&next.revision),
        next.offset
    );
    URL_SAFE.encode(body)
}

/// Python's non-validating `base64.urlsafe_b64decode`: `-` and `_` stand for `+` and `/`,
/// bytes outside the alphabet are dropped, a `=` counts only after two data characters and
/// ends the data once the quad is complete, and the data must end on a complete quad.
/// Text that is not ASCII is refused, as Python refuses it.
fn lenient_base64(raw: &str) -> Option<Vec<u8>> {
    if !raw.is_ascii() {
        return None;
    }
    let (mut out, mut quad, mut left, mut pads) = (Vec::new(), 0u8, 0u8, 0u8);
    for byte in raw.bytes() {
        if byte == b'=' {
            if quad >= 2 {
                pads += 1;
                if quad + pads >= 4 {
                    return Some(out);
                }
            }
            continue;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        pads = 0;
        match quad {
            0 => left = value,
            1 => {
                out.push(left << 2 | value >> 4);
                left = value & 0xf;
            }
            2 => {
                out.push(left << 4 | value >> 2);
                left = value & 3;
            }
            _ => out.push(left << 6 | value),
        }
        quad = (quad + 1) % 4;
    }
    (quad == 0).then_some(out)
}

/// A cursor that is not `[task, revision, offset]` with a non-negative integer offset is
/// malformed, as the reference reads it (`json.loads`, any integer size, `-0` is 0). An
/// offset beyond `i64::MAX` saturates: it is refused against the manifest after the binding.
pub fn decode_cursor(raw: &str) -> ArtifactCursorArg {
    let parsed = (raw.chars().count() <= CURSOR_CHARS)
        .then(|| lenient_base64(raw))
        .flatten()
        .and_then(|bytes| parse(&bytes));
    let Some(Json::Array(parts)) = parsed else {
        return ArtifactCursorArg::Malformed;
    };
    let text = |codes: &[u32]| -> String {
        codes
            .iter()
            .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
            .collect()
    };
    match parts.as_slice() {
        [
            Json::Text(task),
            Json::Text(revision),
            Json::Int { negative, digits },
        ] if !*negative || digits == "0" => ArtifactCursorArg::Valid(NextCursor {
            task: text(task),
            revision: text(revision),
            offset: digits.parse().unwrap_or(i64::MAX),
        }),
        _ => ArtifactCursorArg::Malformed,
    }
}

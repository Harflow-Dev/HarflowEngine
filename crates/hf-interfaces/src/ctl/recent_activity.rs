//! `recent_activity` of a brief status: the last meaningful events of the log as one-line
//! strings, from the stored event bodies. Streamed text merges into one line, a successful
//! tool result says nothing its call did not, and a body that does not parse is skipped.
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

const LINES: usize = 3;
const LINE_CHARS: usize = 160;
const MEANINGFUL: [&str; 6] = [
    "tool_call",
    "tool_result",
    "assistant_text",
    "user_message",
    "notice",
    "undelivered",
];

type Event = Map<String, Value>;

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

/// Python's `str(value)` for the values an event field holds.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

/// The first truthy of the named fields, as text.
fn first(event: &Event, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| event.get(*key))
        .find(|value| truthy(value))
        .map(text_of)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn kind(event: &Event) -> &str {
    event.get("kind").and_then(Value::as_str).unwrap_or("")
}

/// A streamed block is identified by a message id and a block index together.
fn identity(event: &Event) -> Option<(String, i64)> {
    let message = event
        .get("message_id")?
        .as_str()
        .filter(|id| !id.is_empty())?;
    Some((message.to_string(), event.get("block_index")?.as_i64()?))
}

/// The block still being streamed: the position of its newest delta and its text so far.
/// Only the newest identified block counts, and only while its finished text has not arrived.
fn streaming(events: &[Event]) -> Option<(usize, String)> {
    let finished: HashSet<(String, i64)> = events
        .iter()
        .filter(|event| kind(event) == "assistant_text")
        .filter_map(identity)
        .collect();
    let position = events
        .iter()
        .rposition(|event| kind(event) == "assistant_delta" && identity(event).is_some())?;
    let wanted = identity(&events[position])?;
    if finished.contains(&wanted) {
        return None;
    }
    let text = events[..=position]
        .iter()
        .filter(|event| {
            kind(event) == "assistant_delta" && identity(event).as_ref() == Some(&wanted)
        })
        .filter_map(|event| event.get("text").and_then(Value::as_str))
        .collect();
    Some((position, text))
}

fn line_of(event: &Event, names: &HashMap<String, String>) -> Option<String> {
    let text = |keys: &[&str]| one_line(&first(event, keys).unwrap_or_default());
    let line = match kind(event) {
        "tool_call" => format!(
            "{}  {}  {}",
            first(event, &["category"]).unwrap_or_else(|| "tool".to_string()),
            first(event, &["tool"]).unwrap_or_default(),
            text(&["command", "path", "input_preview"]),
        ),
        "tool_result" => {
            if event.get("ok").is_none_or(truthy) {
                return None;
            }
            let call = event.get("call_id").map(text_of).unwrap_or_default();
            let name = names.get(&call).map_or("a tool call", String::as_str);
            let output = first(event, &["output_tail"]).unwrap_or_default();
            match output.lines().find(|line| !line.trim().is_empty()) {
                Some(line) => format!("failed  {name}: {}", one_line(line)),
                None => format!("failed  {name}"),
            }
        }
        "assistant_text" => format!("text  {}", text(&["text"])),
        "streaming" => {
            let words = text(&["text"]);
            let room = LINE_CHARS - "text…  …".chars().count();
            let count = words.chars().count();
            if count > room {
                let tail: String = words.chars().skip(count - room).collect();
                format!("text…  …{tail}")
            } else {
                format!("text…  {words}")
            }
        }
        "user_message" => format!("user  {}", text(&["text"])),
        "notice" => format!("notice  {}", text(&["text"])),
        "undelivered" => format!("undelivered  {}", text(&["text", "reason"])),
        _ => return None,
    };
    let line = line.trim_end();
    if line.chars().count() > LINE_CHARS {
        let kept: String = line.chars().take(LINE_CHARS - 1).collect();
        return Some(format!("{kept}…"));
    }
    Some(line.to_string())
}

/// The last three lines, oldest first, from bodies given oldest first.
pub fn recent_lines(bodies: &[Vec<u8>]) -> Vec<String> {
    let events: Vec<Event> = bodies
        .iter()
        .filter_map(|body| match serde_json::from_slice(body) {
            Ok(Value::Object(event)) => Some(event),
            _ => None,
        })
        .collect();
    let names: HashMap<String, String> = events
        .iter()
        .filter(|event| kind(event) == "tool_call")
        .filter_map(|event| {
            let call = event.get("call_id").filter(|value| truthy(value))?;
            let tool = first(event, &["tool"]).unwrap_or_else(|| "a tool call".to_string());
            Some((text_of(call), tool))
        })
        .collect();
    let live = streaming(&events);
    let mut lines: Vec<String> = Vec::new();
    for (position, event) in events.iter().enumerate() {
        if kind(event) == "assistant_delta" {
            if let Some((_, words)) = live.as_ref().filter(|(at, _)| *at == position) {
                let mut merged = Event::new();
                merged.insert("kind".into(), Value::String("streaming".to_string()));
                merged.insert("text".into(), Value::String(words.clone()));
                lines.extend(line_of(&merged, &names));
            }
        } else if MEANINGFUL.contains(&kind(event)) {
            lines.extend(line_of(event, &names));
        }
    }
    let skip = lines.len().saturating_sub(LINES);
    lines.split_off(skip)
}

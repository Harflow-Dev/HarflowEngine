//! The wire cursors of `interaction-events` and `statistics-page`: URL-safe base64 of compact
//! JSON, read as the reference reads them. Neither is bound to a task or event generation.
use super::{event_cursor::reference_decode, py_json::parse, py_value::Json};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use hf_application::ctl::task_reads::{FeedCursorArg, StatsCursorArg};
use hf_domain::ctl::{
    feed::{FIELD_CHARS, FeedState, Phase, identifier_ok},
    statistics_time::Bounds,
};

const FEED_CURSOR_CHARS: usize = 4096;
const STATS_CURSOR_BYTES: usize = 4096;

/// The object a cursor text decodes to, with exactly `keys` as members.
fn object(raw: &str, keys: &[&str]) -> Option<Json> {
    let json = parse(&reference_decode(raw)?)?;
    let mut found = json.keys()?;
    found.sort();
    let mut wanted: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
    wanted.sort();
    (found == wanted).then_some(json)
}

/// A state field: text of at most 512 code points.
fn field(json: &Json, key: &str) -> Option<String> {
    let value = json.get(key)?;
    (value.code_points()? <= FIELD_CHARS).then(|| value.text())?
}

fn feed_state(raw: &str) -> Option<FeedState> {
    if raw.chars().count() > FEED_CURSOR_CHARS {
        return None;
    }
    let json = object(raw, &["v", "phase", "after", "source", "event"])?;
    if !json.get("v")?.equals_one() {
        return None;
    }
    let phase = match json.get("phase")?.text()?.as_str() {
        "workflow" => Phase::Workflow,
        "task" => Phase::Task,
        _ => return None,
    };
    let (after, source, event) = (
        field(&json, "after")?,
        field(&json, "source")?,
        field(&json, "event")?,
    );
    let named = |text: &String| text.is_empty() || identifier_ok(text);
    (named(&after) && named(&source)).then_some(FeedState {
        phase,
        after,
        source,
        event,
    })
}

pub fn decode_feed(raw: &str) -> FeedCursorArg {
    feed_state(raw).map_or(FeedCursorArg::Malformed, FeedCursorArg::State)
}

/// A JSON string as Python writes it by default: every non-ASCII character escaped.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

pub fn encode_feed(state: &FeedState) -> String {
    let phase = match state.phase {
        Phase::Workflow => "workflow",
        Phase::Task => "task",
    };
    let body = format!(
        "{{\"v\":1,\"phase\":\"{phase}\",\"after\":{},\"source\":{},\"event\":{}}}",
        quoted(&state.after),
        quoted(&state.source),
        quoted(&state.event)
    );
    URL_SAFE.encode(body)
}

const STATS_KEYS: [&str; 6] = ["v", "root", "since", "before", "generation", "offset"];

/// Judges a statistics cursor against the root and the window of the request. `bounds` is
/// `None` when the request has none, in which case the request fails before the cursor
/// matters.
pub fn decode_statistics(raw: &str, root: &str, bounds: Option<&Bounds>) -> StatsCursorArg {
    if raw.len() > STATS_CURSOR_BYTES {
        return StatsCursorArg::Malformed;
    }
    let Some(json) = object(raw, &STATS_KEYS) else {
        return StatsCursorArg::Malformed;
    };
    let version = json.get("v").is_some_and(Json::equals_one);
    let offset = json.get("offset").and_then(Json::count);
    let (true, Some(offset)) = (version, offset) else {
        return StatsCursorArg::Malformed;
    };
    let same = |key: &str, number: i64| json.get(key).is_some_and(|v| v.equals_integer(number));
    let bound = bounds.is_some_and(|b| same("since", b.since_us) && same("before", b.before_us));
    if !(bound && json.get("root").is_some_and(|v| v.equals_text(root))) {
        return StatsCursorArg::Foreign;
    }
    StatsCursorArg::Bound {
        generation: json.get("generation").and_then(Json::text),
        offset,
    }
}

pub fn encode_statistics(root: &str, bounds: &Bounds, generation: &str, offset: usize) -> String {
    let body = format!(
        "{{\"v\":1,\"root\":{},\"since\":{},\"before\":{},\"generation\":{},\"offset\":{offset}}}",
        quoted(root),
        bounds.since_us,
        bounds.before_us,
        quoted(generation)
    );
    URL_SAFE.encode(body)
}

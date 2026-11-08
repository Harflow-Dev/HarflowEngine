//! The wire form of an event cursor: URL-safe base64 of compact JSON, opaque to clients.
//!
//! Well-formedness comes before any generation answer. This engine's own cursors carry
//! `gen: "rs-events-1"` and every `EventCursor` field. A cursor of another generation is
//! checked against the complete shape of the reference engine's cursor; only its task and
//! filter digests are read from it, so the application can tell an expired cursor of this
//! task and filter from an invalid one.
use base64::{
    Engine, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig, general_purpose::URL_SAFE_NO_PAD},
};
use hf_application::ctl::task_reads::EventCursorArg;
use hf_domain::{
    event_read::{Direction, EventCursor},
    task_read::TaskId,
};
use serde_json::{Map, Value, json};

const MAX_CHARS: usize = 2048;
const VERSION: u64 = 2;
/// The generation of this engine's cursors.
pub const GENERATION: &str = "rs-events-1";
const GENERATION_CHARS: usize = 32;
const ANCHOR_CHARS: usize = 64;

pub fn encode(cursor: &EventCursor) -> String {
    let direction = match cursor.direction {
        Direction::Older => "older",
        Direction::Newer => "newer",
    };
    let body = json!({
        "v": VERSION,
        "gen": GENERATION,
        "store": cursor.store,
        "stream": cursor.generation,
        "task": cursor.task.as_str(),
        "kinds": cursor.kinds,
        "dir": direction,
        "boundary": cursor.boundary,
        "resync": cursor.resync,
        "seq": cursor.neighbour_seq,
        "skip": cursor.neighbour_skip,
    });
    URL_SAFE_NO_PAD.encode(body.to_string())
}

pub fn decode(raw: &str) -> EventCursorArg {
    classify(raw).unwrap_or(EventCursorArg::Malformed)
}

/// The reference engine's decoder, `base64.b64decode(cursor, altchars=b"-_", validate=True)`:
/// `-` and `_` stand for `+` and `/`, so both alphabets are read; padding is required and
/// exact; nothing else may appear, whitespace included; non-zero trailing bits are allowed.
pub(super) fn reference_decode(raw: &str) -> Option<Vec<u8>> {
    let engine = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    let translated: String = raw
        .chars()
        .map(|ch| match ch {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    engine.decode(translated).ok()
}

/// The cursor's object, and whether the reference decoder reads it. This engine's own
/// cursors are written without padding and never satisfy it; only a cursor of another
/// generation must.
fn object(raw: &str) -> Option<(Map<String, Value>, bool)> {
    if raw.is_empty() || raw.chars().count() > MAX_CHARS {
        return None;
    }
    let (bytes, padded) = match reference_decode(raw) {
        Some(bytes) => (bytes, true),
        None => (URL_SAFE_NO_PAD.decode(raw).ok()?, false),
    };
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(map)) => Some((map, padded)),
        _ => None,
    }
}

fn classify(raw: &str) -> Option<EventCursorArg> {
    let (map, padded) = object(raw)?;
    if map.get("v").and_then(Value::as_u64) != Some(VERSION) {
        return None;
    }
    let generation = map.get("gen")?.as_str()?;
    if generation == GENERATION {
        return native(&map).map(EventCursorArg::Native);
    }
    if padded {
        foreign(&map, generation)
    } else {
        None
    }
}

fn flag(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key)?.as_bool()
}

fn native(map: &Map<String, Value>) -> Option<EventCursor> {
    const KEYS: [&str; 11] = [
        "v", "gen", "store", "stream", "task", "kinds", "dir", "boundary", "resync", "seq", "skip",
    ];
    if map.len() != KEYS.len() || KEYS.iter().any(|key| !map.contains_key(*key)) {
        return None;
    }
    let direction = match map.get("dir")?.as_str()? {
        "older" => Direction::Older,
        "newer" => Direction::Newer,
        _ => return None,
    };
    let kinds = map
        .get("kinds")?
        .as_array()?
        .iter()
        .map(|kind| kind.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    let neighbour_seq = match map.get("seq")? {
        Value::Null => None,
        other => Some(other.as_u64()?),
    };
    Some(EventCursor {
        store: map.get("store")?.as_str()?.to_string(),
        generation: map.get("stream")?.as_str()?.to_string(),
        task: TaskId::new(map.get("task")?.as_str()?.to_string()).ok()?,
        kinds,
        direction,
        boundary: map.get("boundary")?.as_u64()?,
        resync: flag(map, "resync")?,
        neighbour_seq,
        neighbour_skip: flag(map, "skip")?,
    })
}

/// The reference engine's cursor shape, every type and range: `dev`, `ino` and `boundary`
/// are integers below 2^64, `hlen` is 0..=64, five fields are 64-character strings, the
/// direction is `older` or `newer`, `resync` and `skip` are booleans and `seq` is null or an
/// integer below 2^63.
fn foreign(map: &Map<String, Value>, generation: &str) -> Option<EventCursorArg> {
    if generation.chars().count() > GENERATION_CHARS {
        return None;
    }
    for key in ["dev", "ino", "boundary"] {
        map.get(key)?.as_u64()?;
    }
    if map.get("hlen")?.as_u64()? > 64 {
        return None;
    }
    let anchor = |key: &str| {
        let text = map.get(key)?.as_str()?;
        (text.chars().count() == ANCHOR_CHARS).then(|| text.to_string())
    };
    let (task, filter) = (anchor("task")?, anchor("filter")?);
    for key in ["path", "anchor", "head"] {
        anchor(key)?;
    }
    if !["older", "newer"].contains(&map.get("dir")?.as_str()?) {
        return None;
    }
    flag(map, "resync")?;
    flag(map, "skip")?;
    match map.get("seq")? {
        Value::Null => {}
        other => {
            if other.as_u64()? >= 1 << 63 {
                return None;
            }
        }
    }
    Some(EventCursorArg::Foreign { task, filter })
}

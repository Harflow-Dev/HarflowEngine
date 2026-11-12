//! The wire form of a pending page cursor: padded URL-safe base64 of compact JSON with
//! sorted keys, opaque to clients. It binds the position to the state root, the store, the
//! launch namespace, the page size and the uncertainty seen so far.
use super::{py_json::parse, py_value::Json};
use base64::{
    Engine,
    alphabet::STANDARD,
    engine::{
        DecodePaddingMode,
        general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE},
    },
};
use hf_application::ctl::task_reads::{PendingCursor, PendingCursorArg};
use hf_domain::{
    ctl::pending::{MAX_CURSOR_BYTES, MAX_LIMIT, SCOPE},
    task_read::TaskId,
};
use serde_json::json;

const KEYS: [&str; 8] = [
    "v",
    "scope",
    "root",
    "identity",
    "revision",
    "last",
    "limit",
    "uncertain",
];

pub(super) fn encode(cursor: &PendingCursor) -> String {
    let identity: Vec<u64> = cursor
        .identity
        .iter()
        // The identity parts are decimal text of whole numbers by construction; a part that
        // were not would encode as 0 and fail the binding check on the next read.
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let body = json!({
        "v": 1,
        "scope": SCOPE,
        "root": cursor.root,
        "identity": identity,
        "revision": cursor.revision,
        "last": cursor.last,
        "limit": cursor.limit,
        "uncertain": cursor.uncertain,
    });
    URL_SAFE.encode(body.to_string())
}

/// Python's validating `base64.b64decode(..., altchars=b'-_')`: both alphabets are accepted,
/// the text must be ASCII, and the padding must be complete.
fn strict_base64(raw: &str) -> Option<Vec<u8>> {
    if !raw.is_ascii() {
        return None;
    }
    let text: String = raw
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let data = text.trim_end_matches('=');
    let pads = text.len() - data.len();
    let shaped = pads <= 2
        && data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/');
    let config = GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical);
    shaped
        .then(|| GeneralPurpose::new(&STANDARD, config).decode(text).ok())
        .flatten()
}

fn hex64(value: Option<&Json>) -> Option<String> {
    let text = value?.text()?;
    (text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then_some(text)
}

/// The decimal spelling of a whole number, `-0` being 0.
fn whole(value: &Json) -> Option<String> {
    let Json::Int { negative, digits } = value else {
        return None;
    };
    Some(if *negative && digits != "0" {
        format!("-{digits}")
    } else {
        digits.clone()
    })
}

fn identity(value: Option<&Json>) -> Option<[String; 4]> {
    let Some(Json::Array(parts)) = value else {
        return None;
    };
    let parts: Vec<String> = parts.iter().map(whole).collect::<Option<_>>()?;
    <[String; 4]>::try_from(parts).ok()
}

fn limit(value: Option<&Json>) -> Option<i64> {
    let number: i64 = whole(value?)?.parse().ok()?;
    (1..=MAX_LIMIT).contains(&number).then_some(number)
}

fn fields(object: &Json) -> Option<PendingCursor> {
    let mut keys = object.keys()?;
    keys.sort();
    keys.dedup();
    let mut wanted = KEYS.to_vec();
    wanted.sort_unstable();
    if keys != wanted || whole(object.get("v")?)? != "1" || !object.get("scope")?.equals_text(SCOPE)
    {
        return None;
    }
    let last = object.get("last")?.text()?;
    TaskId::new(last.clone()).ok()?;
    let Some(Json::Bool(uncertain)) = object.get("uncertain") else {
        return None;
    };
    Some(PendingCursor {
        root: hex64(object.get("root"))?,
        identity: identity(object.get("identity"))?,
        revision: hex64(object.get("revision"))?,
        last,
        limit: limit(object.get("limit"))?,
        uncertain: *uncertain,
    })
}

/// Anything that is not a cursor of this family is malformed.
pub(super) fn decode(raw: &str) -> PendingCursorArg {
    let value = (raw.len() <= MAX_CURSOR_BYTES)
        .then(|| strict_base64(raw))
        .flatten()
        .and_then(|bytes| parse(&bytes));
    value
        .as_ref()
        .and_then(fields)
        .map_or(PendingCursorArg::Malformed, PendingCursorArg::Valid)
}

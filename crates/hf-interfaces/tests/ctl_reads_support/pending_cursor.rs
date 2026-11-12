//! The scripted world and helpers of the pending cursor tests.
use super::{Fake, args, fake};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use hf_application::ctl::task_reads::Namespace;
use hf_domain::{ctl::caller::PidObservation, ctl::pending::LaunchFacts};
use hf_interfaces::ctl::dispatch::{Outcome, dispatch};
use serde_json::Value;

pub const SCOPE: &str = "prerecord_launch_attempts_and_not_started";
pub const FIRST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub fn row(id: &str) -> LaunchFacts {
    LaunchFacts {
        id: id.into(),
        state: Some("reserved".into()),
        generation: Some("c".repeat(32)),
        backend: Some("claude".into()),
        text_valid: true,
        ..LaunchFacts::default()
    }
}

pub fn world() -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.pending.rows = vec![row(FIRST), row(SECOND)];
    fake.pending.namespace = Ok(Namespace {
        revision: "a".repeat(64),
        invalid: 0,
    });
    fake
}

/// The members in the order of the reference's sorted keys.
pub fn body(edit: impl Fn(String) -> String) -> String {
    edit(format!(
        "{{\"identity\":[1,2,1,3],\"last\":\"{FIRST}\",\"limit\":2,\"revision\":\"{}\",\"root\":\"{}\",\"scope\":\"{SCOPE}\",\"uncertain\":false,\"v\":1}}",
        "a".repeat(64),
        "f".repeat(64)
    ))
}

pub fn good() -> String {
    body(|text| text)
}

pub fn page(fake: &Fake, cursor: &str, limit: &str) -> Outcome {
    dispatch(
        &args(&[
            "task-pending-page",
            "--limit",
            limit,
            "--cursor",
            cursor,
            "--json",
        ]),
        fake,
    )
}

pub fn error(out: &Outcome) -> (String, String) {
    assert_eq!(out.code, 1, "{}", out.stdout);
    let value: Value = serde_json::from_str(&out.stdout).unwrap();
    (
        value["error"]["code"].as_str().unwrap().to_owned(),
        value["error"]["data"]["check"].as_str().unwrap().to_owned(),
    )
}

pub fn shape() -> (String, String) {
    ("pending_cursor_invalid".into(), "cursor_shape".into())
}

pub fn accepted(fake: &Fake, cursor: &str) -> bool {
    let out = page(fake, cursor, "2");
    if out.code == 0 {
        let value: Value = serde_json::from_str(&out.stdout).unwrap();
        let ids: Vec<&str> = value["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["task_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, [SECOND], "the walk resumes after the cursor position");
        return true;
    }
    assert_eq!(error(&out), shape(), "{cursor}");
    false
}

pub fn url(text: &str) -> String {
    URL_SAFE.encode(text)
}

pub fn replaced(from: &str, to: &str) -> String {
    let edited = body(|text| text.replace(from, to));
    assert_ne!(edited, good(), "{from}");
    url(&edited)
}

pub fn error_check(out: &Outcome) -> String {
    if out.code == 0 {
        return String::new();
    }
    error(out).1
}

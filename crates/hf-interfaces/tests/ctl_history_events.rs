#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `task-events` document over a scripted store: rendering bounds, cursor codec, and the
//! error documents. Sizes are those of Python's `json.dumps`.
mod ctl_reads_support;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ctl_reads_support::*;
use hf_domain::{
    ctl::{
        caller::PidObservation,
        task_rows::{CallerRow, ProcessRecord},
    },
    event_read::{Direction, EventCursor},
    task_read::TaskId,
};
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const STREAM: &str = "fedcba9876543210fedcba9876543210";

fn settled() -> Fake {
    fake("completed", Some(0), PidObservation::Absent)
}

fn run(fake: &Fake, extra: &[&str]) -> (i32, Value, String) {
    let mut list = vec!["task-events", ID, "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), fake);
    let body = serde_json::from_str(&out.stdout).unwrap_or(Value::Null);
    (out.code, body, out.stderr)
}

fn with_bodies(fake: &mut Fake, bodies: Vec<String>) {
    fake.history.pages = Box::new(move |query| {
        let mut page = page_of(&query.task);
        let mut start = 0;
        for (seq, body) in bodies.iter().enumerate() {
            let row = event_row(start, seq as u64, "notice", body);
            start = row.end;
            page.events.push(row);
        }
        page.decoded = page.events.len();
        page.span = Some((0, bodies.len() as u64 - 1));
        Ok(page)
    });
}

fn first_event(fake: &Fake) -> Value {
    let (code, body, stderr) = run(fake, &[]);
    assert_eq!(code, 0, "{stderr}");
    body["result"]["events"][0].clone()
}

fn body_of(fields: Value) -> String {
    let mut base = json!({
        "product": "harflow", "format": "harflow.event", "v": 1, "product_version": "stale",
        "seq": 99, "kind": "other", "observed_at": "2026-01-01T00:00:00+00:00", "task_id": ID,
        "raw_offset": 7, "source_ts": "2020"
    });
    base.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    base.to_string()
}

/// Python's `json.dumps(value, ensure_ascii=False)` size.
fn size(value: &Value) -> usize {
    match value {
        Value::Array(items) => {
            2 + items.iter().map(size).sum::<usize>() + 2 * items.len().saturating_sub(1)
        }
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, v)| size(&json!(k)) + 2 + size(v))
                .sum::<usize>()
                + 2 * map.len().saturating_sub(1)
        }
        other => other.to_string().len(),
    }
}

#[test]
fn a_stored_body_is_restamped_and_stripped_never_served_as_it_is() {
    let mut fake = settled();
    with_bodies(&mut fake, vec![body_of(json!({"text": "hello"}))]);
    let event = first_event(&fake);
    let object = event.as_object().unwrap();
    let mut keys: Vec<&String> = object.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "format",
            "kind",
            "observed_at",
            "product",
            "product_version",
            "seq",
            "text",
            "v"
        ]
    );
    assert_ne!(object["product_version"], "stale");
    assert_eq!(
        (object["seq"].clone(), object["kind"].clone()),
        (json!(0), json!("notice")),
        "seq and kind come from the record"
    );
    assert_eq!(object["text"], "hello");
}

#[test]
fn strings_lists_and_depth_are_cut_at_their_bounds() {
    let mut fake = settled();
    let cases = [
        (
            json!({"s": "x".repeat(2000)}),
            json!("x".repeat(2000)),
            false,
        ),
        (
            json!({"s": "x".repeat(2001)}),
            json!(format!("{}...", "x".repeat(2000))),
            true,
        ),
        (
            json!({"s": "\u{e9}".repeat(2001)}),
            json!(format!("{}...", "\u{e9}".repeat(2000))),
            true,
        ),
        (json!({"s": vec![1; 50]}), json!(vec![1; 50]), false),
        (json!({"s": vec![1; 51]}), json!(vec![1; 50]), true),
        (
            json!({"s": {"b": {"c": {"d": "leaf"}}}}),
            json!({"b": {"c": {"d": "leaf"}}}),
            false,
        ),
        (
            json!({"s": {"b": {"c": {"d": {"e": "leaf"}}}}}),
            json!({"b": {"c": {"d": {"e": "..."}}}}),
            true,
        ),
        (json!({"s": [[["deep"]]]}), json!([[["deep"]]]), false),
        (json!({"s": [[[["deeper"]]]]}), json!([[[["..."]]]]), true),
    ];
    for (fields, expected, cut) in cases {
        with_bodies(&mut fake, vec![body_of(fields.clone())]);
        let event = first_event(&fake);
        assert_eq!(event["s"], expected, "{fields}");
        assert_eq!(event.get("truncated").is_some(), cut, "{fields}");
    }
}

#[test]
fn observed_at_is_null_unless_a_string_of_at_most_one_hundred_and_twenty_eight_characters() {
    let mut fake = settled();
    for (value, expected) in [
        (json!("t".repeat(128)), json!("t".repeat(128))),
        (json!("t".repeat(129)), Value::Null),
        (json!(5), Value::Null),
        (Value::Null, Value::Null),
        (json!({"a": 1}), Value::Null),
    ] {
        with_bodies(&mut fake, vec![body_of(json!({"observed_at": value}))]);
        assert_eq!(first_event(&fake)["observed_at"], expected, "{value}");
    }
}

#[test]
fn a_rendered_event_over_sixteen_kibibytes_is_a_stub_exactly_at_the_boundary() {
    let mut fake = settled();
    let mut last_kept = None;
    for fill in 0..=2000 {
        let mut fields = serde_json::Map::new();
        for index in 0..8 {
            fields.insert(format!("f{index}"), json!("a".repeat(1_900)));
        }
        fields.insert("g".into(), json!("a".repeat(fill)));
        with_bodies(&mut fake, vec![body_of(Value::Object(fields))]);
        let event = first_event(&fake);
        if event.get("oversized").is_some() {
            assert_eq!(event["bytes"], 16_385, "the first stub is one byte over");
            assert_eq!(event["truncated"], true);
            let mut keys: Vec<&String> = event.as_object().unwrap().keys().collect();
            keys.sort();
            assert_eq!(
                keys,
                [
                    "bytes",
                    "format",
                    "kind",
                    "observed_at",
                    "oversized",
                    "product",
                    "product_version",
                    "seq",
                    "truncated",
                    "v"
                ]
            );
            assert_eq!(event["seq"], 0);
            assert_eq!(
                size(&last_kept.unwrap()),
                16_384,
                "the last event kept was exactly the bound"
            );
            return;
        }
        last_kept = Some(event);
    }
    panic!("no stub found");
}

#[test]
fn a_stored_body_that_is_not_an_object_is_reported_not_served() {
    for body in ["[1]", "5", "\"x\"", "{not json"] {
        let mut fake = settled();
        with_bodies(&mut fake, vec![body.to_string()]);
        let (code, doc, _) = run(&fake, &[]);
        assert_eq!(code, 1, "{body}");
        assert_eq!(doc["error"]["code"], "state_incompatible", "{body}");
    }
}

fn cursor(direction: Direction) -> EventCursor {
    EventCursor {
        store: STORE.into(),
        generation: STREAM.into(),
        task: TaskId::new(ID.into()).unwrap(),
        kinds: vec!["notice".into(), "usage".into()],
        direction,
        boundary: 12_345,
        resync: true,
        neighbour_seq: None,
        neighbour_skip: true,
    }
}

#[test]
fn a_cursor_is_unpadded_base64_of_the_documented_object_and_comes_back_unchanged() {
    let older = cursor(Direction::Older);
    let mut fake = settled();
    let issued = older.clone();
    fake.history.pages = Box::new(move |query| {
        let mut page = page_of(&query.task);
        page.older_cursor = Some(issued.clone());
        page.has_older = true;
        Ok(page)
    });
    let (code, doc, _) = run(&fake, &["--kinds", "notice,usage"]);
    assert_eq!(code, 0);
    let text = doc["result"]["older_cursor"].as_str().unwrap();
    assert!(!text.contains('='));
    let body: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(text).unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"v": 2, "gen": "rs-events-1", "store": STORE, "stream": STREAM, "task": ID,
               "kinds": ["notice", "usage"], "dir": "older", "boundary": 12_345, "resync": true,
               "seq": null, "skip": true})
    );
    let (code, _, stderr) = run(&fake, &["--kinds", "notice,usage", "--cursor", text]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fake.history.queries.borrow().last().unwrap().cursor,
        Some(older)
    );
}

#[test]
fn newer_cursors_and_sequence_numbers_survive_the_round_trip() {
    let mut newer = cursor(Direction::Newer);
    newer.neighbour_seq = Some(i64::MAX as u64);
    let mut fake = settled();
    let issued = newer.clone();
    fake.history.pages = Box::new(move |query| {
        let mut page = page_of(&query.task);
        page.newer_cursor = Some(issued.clone());
        Ok(page)
    });
    let (_, doc, _) = run(&fake, &["--kinds", "notice,usage"]);
    let text = doc["result"]["newer_cursor"].as_str().unwrap().to_string();
    let body: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&text).unwrap()).unwrap();
    assert_eq!(
        (body["dir"].clone(), body["seq"].clone()),
        (json!("newer"), json!(i64::MAX))
    );
    run(&fake, &["--kinds", "notice,usage", "--cursor", &text]);
    assert_eq!(
        fake.history.queries.borrow().last().unwrap().cursor,
        Some(newer)
    );
}

#[test]
fn cursor_failures_carry_the_reload_hint_and_their_own_code() {
    let mut fake = settled();
    let (code, doc, stderr) = run(&fake, &["--cursor", "garbage"]);
    assert_eq!(code, 1);
    let error = &doc["error"];
    assert_eq!(error["code"], "event_cursor_invalid");
    assert_eq!(error["data"], json!({"reload": "newest"}));
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .ends_with("; reload the newest page")
    );
    assert_eq!(
        stderr,
        format!(
            "harflow-ctl: error: {}\n",
            error["message"].as_str().unwrap()
        )
    );
    let foreign = base64::engine::general_purpose::URL_SAFE.encode(
        json!({"v": 2, "gen": "py-events-1", "dir": "older", "task": "t".repeat(64), "dev": 1, "ino": 2,
               "path": "p".repeat(64), "boundary": 3, "anchor": "a".repeat(64), "head": "h".repeat(64),
               "hlen": 64, "filter": "f".repeat(64), "resync": false, "seq": null, "skip": false})
        .to_string(),
    );
    fake.history.binds = true;
    let (_, doc, _) = run(&fake, &["--cursor", &foreign]);
    assert_eq!(doc["error"]["code"], "event_cursor_expired");
    assert_eq!(
        doc["error"]["message"],
        "Event cursor generation is no longer served; reload the newest page"
    );
    fake.history.binds = false;
    let (_, doc, _) = run(&fake, &["--cursor", &foreign]);
    assert_eq!(doc["error"]["code"], "event_cursor_invalid");
}

fn agent_fake(seen: PidObservation) -> Fake {
    let mut fake = fake("running", None, seen);
    fake.related = vec![CallerRow {
        task: TaskId::new(OTHER.into()).unwrap(),
        status: "running".into(),
        exit_code: None,
        process: ProcessRecord {
            pid: 40,
            start: Some(identity(40)),
        },
    }];
    fake
}

#[test]
fn history_refusals_are_worded_for_the_path_and_carry_no_task_id() {
    let cases = [
        (PidObservation::Present(identity(40)), "agent_caller"),
        (PidObservation::Unknown, "caller_undecidable"),
    ];
    for (seen, code) in cases {
        let fake = agent_fake(seen);
        for command in ["task-events", "task-prompt"] {
            let out = dispatch(&args(&[command, ID, "--json"]), &fake);
            let doc: Value = serde_json::from_str(&out.stdout).unwrap();
            assert_eq!(
                (out.code, doc["error"]["code"].as_str()),
                (1, Some(code)),
                "{command}"
            );
            assert!(
                doc["error"]["message"]
                    .as_str()
                    .unwrap()
                    .starts_with("Task history reads require a verified direct caller")
            );
            assert!(!out.stdout.contains(ID) && !out.stderr.contains(ID));
        }
    }
}

#[test]
fn a_damaged_root_is_an_undecidable_caller_for_history_and_an_artifact_error_for_artifacts() {
    let mut fake = settled();
    fake.damaged = true;
    for command in ["task-events", "task-prompt"] {
        let out = dispatch(&args(&[command, ID, "--json"]), &fake);
        let doc: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(doc["error"]["code"], "caller_undecidable", "{command}");
    }
    let out = dispatch(&args(&["task-artifacts", ID, "--json"]), &fake);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(doc["error"]["code"], "artifact_error");
}

#[test]
fn a_store_failure_from_a_page_is_reported_with_its_own_code() {
    let mut fake = settled();
    fake.history.fail = Some(hf_application::ctl::task_reads::StoreFailure::Busy);
    let (code, doc, _) = run(&fake, &[]);
    assert_eq!(
        (code, doc["error"]["code"].as_str()),
        (1, Some("state_busy"))
    );
}

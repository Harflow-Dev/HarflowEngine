#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_interfaces::event_consumer::{Document, PreparedLine, prepare_line};
use hf_interfaces::task_consumer::{DecodeError, Field, VersionLine};
use serde_json::json;

fn event(seq: u64, kind: &str, version: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "product":"harflow", "format":"harflow.event", "v":1,
        "product_version":version, "seq":seq, "kind":kind,
        "observed_at":"2026-01-01T00:00:00Z", "future_field":{"x":true}
    }))
    .unwrap()
}

fn ctl_page(events: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format":"harflow.ctl", "product":"harflow", "product_version":"1.2.3", "v":1,
        "result":{"events":events,"future_page_field":9}
    }))
    .unwrap()
}

#[test]
fn preparation_preserves_unknown_kind_fields_and_historical_inner_versions() {
    let line = event(7, "future_event_kind", "0.0.8");
    let PreparedLine::Event { seq, kind, body } = prepare_line(&line) else {
        panic!("valid historical event")
    };
    assert_eq!(seq, 7);
    assert_eq!(kind, "future_event_kind");
    assert_eq!(body["product_version"], "0.0.8");
    assert_eq!(body["future_field"]["x"], true);
}

#[test]
fn preparation_classifies_empty_invalid_utf8_and_oversized_records() {
    assert_eq!(prepare_line(b""), PreparedLine::Blank);
    assert_eq!(prepare_line(&[0xff, 0xfe]), PreparedLine::Malformed);
    let mut too_large = event(1, "assistant_text", "1.2.3");
    too_large.resize(1_000_001, b' ');
    assert_eq!(prepare_line(&too_large), PreparedLine::Oversized);
}

#[test]
fn preparation_rejects_invalid_identity_sequence_and_unbounded_kind_metadata() {
    assert_eq!(
        prepare_line(&event(i64::MAX as u64 + 1, "assistant_text", "1.2.3")),
        PreparedLine::Malformed
    );
    assert_eq!(
        prepare_line(&event(1, &"k".repeat(129), "1.2.3")),
        PreparedLine::Malformed
    );
    assert_eq!(prepare_line(br#"{"product":"harflow","format":"harflow.event","v":true,"product_version":"1.2.3","seq":1,"kind":"x"}"#), PreparedLine::Malformed);
    assert_eq!(prepare_line(br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"1.2.3","seq":true,"kind":"x"}"#), PreparedLine::Malformed);
}

#[test]
fn ctl_parser_enforces_complete_document_bound_and_retains_unknown_page_data() {
    let expected = VersionLine::new(1, 2).unwrap();
    let bytes = ctl_page(json!([]));
    let doc = Document::parse(&bytes, expected).unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.field("older_cursor"), Field::Missing);
    assert_eq!(page.raw()["future_page_field"], 9);
    assert_eq!(page.events().unwrap().len(), 0);
    assert_eq!(page.field("missing"), Field::Missing);
    let mut null_page: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    null_page["result"]["older_cursor"] = serde_json::Value::Null;
    let null_doc = Document::parse(&serde_json::to_vec(&null_page).unwrap(), expected).unwrap();
    assert_eq!(null_doc.page().unwrap().field("older_cursor"), Field::Null);
    let over = vec![b' '; 262_145];
    assert_eq!(
        Document::parse(&over, expected).err(),
        Some(DecodeError::Size)
    );
}

#[test]
fn projected_event_budget_counts_utf8_and_json_escaping_with_envelope() {
    let expected = VersionLine::new(1, 2).unwrap();
    let mut near_cap =
        serde_json::from_slice::<serde_json::Value>(&event(1, "assistant_text", "1.2.3")).unwrap();
    // Escaping and multibyte UTF-8 both consume encoded bytes, not character count.
    near_cap["text"] = json!("\\\"é".repeat(5000));
    assert_eq!(
        Document::parse(&ctl_page(json!([near_cap])), expected).err(),
        Some(DecodeError::Size)
    );
}

#[test]
fn ctl_parser_accepts_projection_and_rejects_invalid_nested_event_envelopes() {
    let expected = VersionLine::new(1, 2).unwrap();
    let projected =
        serde_json::from_slice::<serde_json::Value>(&event(3, "unknown_kind", "0.0.8")).unwrap();
    assert!(Document::parse(&ctl_page(json!([projected])), expected).is_ok());
    let mut invalid =
        serde_json::from_slice::<serde_json::Value>(&event(3, "known", "0.0.8")).unwrap();
    invalid["product_version"] = json!("not-a-version");
    assert_eq!(
        Document::parse(&ctl_page(json!([invalid])), expected).err(),
        Some(DecodeError::Version)
    );
}

#[test]
fn ctl_parser_retains_unknown_log_state_and_filter_names_without_inventing_fields() {
    let expected = VersionLine::new(1, 2).unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&ctl_page(json!([]))).unwrap();
    document["result"]["log"] =
        json!({"state":"future_state","capture_state":"future_capture","output_drained":null});
    document["result"]["kinds"] = json!(["future_kind"]);
    let parsed = Document::parse(&serde_json::to_vec(&document).unwrap(), expected).unwrap();
    let page = parsed.page().unwrap();
    assert_eq!(page.raw()["log"]["state"], "future_state");
    assert_eq!(page.raw()["kinds"][0], "future_kind");
    assert_eq!(page.field("has_newer"), Field::Missing);
    assert_eq!(page.raw()["log"]["output_drained"], serde_json::Value::Null);
}

#[test]
fn pinned_ctl_event_documents_and_errors_match_the_frozen_bundle() {
    let expected = VersionLine::new(0, 0).unwrap();
    let fixtures = [
        "ctl-task-events-settling.json",
        "ctl-task-events-cursor-expired.json",
        "ctl-task-events-older.json",
        "ctl-task-events-direct-caller-refused.json",
        "ctl-task-events-all-kinds.json",
        "ctl-task-events-gaps-skipped.json",
        "ctl-task-events-pending-launch.json",
        "ctl-task-events-unknown-task.json",
        "ctl-task-events-unavailable.json",
        "ctl-task-events-oldest-forward.json",
        "ctl-task-events-newest.json",
        "ctl-task-events-cursor-invalid.json",
        "ctl-task-events-open.json",
        "ctl-task-events-incomplete.json",
        "ctl-task-events-kinds-filtered.json",
        "ctl-task-events-default-filter-delta-seqs.json",
        "ctl-task-events-newer.json",
        "ctl-task-events-newer-empty-final.json",
    ];
    for name in fixtures {
        let path = format!("../../tests/fixtures/contracts-v1/{name}");
        let bytes = std::fs::read(path).unwrap();
        assert!(
            Document::parse(&bytes, expected).is_ok(),
            "pinned fixture {name}"
        );
    }
    let gaps: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/contracts-v1/ctl-task-events-gaps-skipped.json"
    ))
    .unwrap();
    let parsed = Document::parse(&serde_json::to_vec(&gaps).unwrap(), expected).unwrap();
    assert_eq!(
        parsed.page().unwrap().raw()["span"],
        json!({"first_seq":0,"last_seq":6})
    );
    assert_eq!(
        parsed.page().unwrap().raw()["gaps"][0],
        json!({"after_seq":1,"before_seq":4,"reason":"unknown"})
    );
    assert_eq!(parsed.page().unwrap().raw()["skipped"]["malformed"], 1);

    let historical =
        include_bytes!("../../../tests/fixtures/contracts-v1/ctl-event-page-historical-inner.json");
    let historical = Document::parse(historical, VersionLine::new(1, 1).unwrap()).unwrap();
    assert_eq!(
        historical.page().unwrap().events().unwrap()[0].raw()["product_version"],
        "1.0.7"
    );
    let invalid =
        include_bytes!("../../../tests/fixtures/contracts-v1/invalid-ctl-inner-event.json");
    assert_eq!(
        Document::parse(invalid, expected).err(),
        Some(DecodeError::Envelope)
    );
}

#[test]
fn fixture_projection_body_retains_identity_metadata_and_applies_bounded_projection() {
    let raw = serde_json::to_vec(&json!({
        "product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8",
        "seq":12,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z",
        "text":"é".repeat(2100)
    }))
    .unwrap();
    let PreparedLine::Event { seq, kind, body } = prepare_line(&raw) else {
        panic!("valid event")
    };
    assert_eq!(body["seq"], seq);
    assert_eq!(body["kind"], kind);
    assert_eq!(body["format"], "harflow.event");
    assert_eq!(body["product_version"], "0.0.8");
    assert_eq!(body["text"].as_str().unwrap().chars().count(), 2003);
    assert_eq!(body["truncated"], true);
}

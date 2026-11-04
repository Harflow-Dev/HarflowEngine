#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_interfaces::task_consumer::{DecodeError, Document, Field, ParseLimits, VersionLine};

fn expected() -> VersionLine {
    VersionLine::new(1, 2).unwrap()
}

fn envelope(payload: &str) -> String {
    format!(
        r#"{{"format":"harflow.ctl","product":"harflow","product_version":"1.2.3","v":1,{payload}}}"#
    )
}

fn parse(payload: &str) -> Result<Document, DecodeError> {
    Document::parse(
        envelope(payload).as_bytes(),
        expected(),
        ParseLimits::default(),
    )
}

#[test]
fn task_fields_keep_missing_null_values_and_unknown_data() {
    let document = parse(
        r#""task":{"task_id":"task_1234","status":null,"active":false,"future_fact":{"x":1}}"#,
    )
    .unwrap();
    let task = document.status().unwrap();

    assert_eq!(task.task_id().unwrap(), "task_1234");
    assert_eq!(task.string("status").unwrap(), Field::Null);
    assert_eq!(task.boolean("active").unwrap(), Field::Value(false));
    assert_eq!(task.boolean("observed_exit").unwrap(), Field::Missing);
    assert!(matches!(task.field("future_fact"), Field::Value(_)));
    assert_eq!(document.raw()["task"]["future_fact"]["x"], 1);
}

#[test]
fn page_items_cursor_and_coverage_preserve_missing_null_and_values() {
    let document = parse(
        r#""result":{"items":[{"task_id":"page-task"}],"next_cursor":null,"has_more":false,"total":0}"#,
    )
    .unwrap();
    let page = document.page().unwrap();
    assert_eq!(page.items().unwrap().len(), 1);
    assert_eq!(page.items().unwrap()[0].task_id().unwrap(), "page-task");
    assert_eq!(page.next_cursor().unwrap(), Field::Null);
    assert_eq!(page.boolean("has_more").unwrap(), Field::Value(false));
    assert_eq!(page.unsigned("total").unwrap(), Field::Value(0));
    assert_eq!(page.field("counts_complete"), Field::Missing);
}

#[test]
fn optional_page_fields_accept_omission_and_opaque_cursor_strings() {
    let missing = parse(r#""result":{"items":[]}"#).unwrap();
    assert_eq!(
        missing.page().unwrap().next_cursor().unwrap(),
        Field::Missing
    );
    let cursor = "c".repeat(2048);
    let payload = format!(r#""result":{{"items":[],"next_cursor":"{cursor}"}}"#);
    let present = parse(&payload).unwrap();
    assert_eq!(
        present.page().unwrap().next_cursor().unwrap(),
        Field::Value(cursor.as_str())
    );
    let oversized = format!(
        r#""result":{{"items":[],"next_cursor":"{}"}}"#,
        "c".repeat(2049)
    );
    assert_eq!(parse(&oversized).err(), Some(DecodeError::Field));
}

#[test]
fn unknown_future_status_strings_and_error_data_are_retained() {
    let status = parse(r#""task":{"task_id":"task-1234","status":"future-state"}"#).unwrap();
    assert_eq!(
        status.status().unwrap().string("status").unwrap(),
        Field::Value("future-state")
    );

    let error = parse(r#""error":{"code":"future_code","message":"opaque","data":null}"#).unwrap();
    let view = error.error().unwrap();
    assert_eq!(view.code().unwrap(), "future_code");
    assert_eq!(view.message().unwrap(), "opaque");
    assert_eq!(view.data(), Field::Null);
    assert!(error.status().is_none());
    assert!(error.page().is_none());

    let missing_data = parse(r#""error":{"code":"e","message":"m"}"#).unwrap();
    assert_eq!(missing_data.error().unwrap().data(), Field::Missing);
}

#[test]
fn root_error_precedes_simultaneous_task_and_page_payloads() {
    let document = parse(
        r#""error":{"code":"e","message":"m"},"task":{"task_id":"task-1234"},"result":{"items":[]}"#,
    )
    .unwrap();
    assert_eq!(document.error().unwrap().code().unwrap(), "e");
    assert!(document.status().is_none());
    assert!(document.page().is_none());
}

#[test]
fn malformed_envelopes_versions_and_selected_field_types_refuse() {
    assert_eq!(
        Document::parse(b"{", expected(), ParseLimits::default()).err(),
        Some(DecodeError::Json)
    );
    assert_eq!(
        parse(r#""task":{"active":"false"}"#).err(),
        Some(DecodeError::Payload)
    );
    assert_eq!(
        parse(r#""task":{"task_id":"task-1234","active":"false"}"#).err(),
        Some(DecodeError::Field)
    );
    assert_eq!(
        parse(r#""task":{"task_id":"task-1234/invalid"}"#).err(),
        Some(DecodeError::Payload)
    );
    assert_eq!(
        parse(r#""result":{"items":{}}"#).err(),
        Some(DecodeError::Payload)
    );
    assert_eq!(
        parse(r#""error":{"code":"x"}"#).err(),
        Some(DecodeError::Payload)
    );
    assert_eq!(
        parse(r#""result":{"items":[],"next_cursor":7}"#).err(),
        Some(DecodeError::Field)
    );
    let mixed = parse(r#""task":{"task_id":"task-1234"},"result":{"items":[]}"#).unwrap();
    assert!(mixed.status().is_some());
    assert!(mixed.page().is_none());
    let prerelease = envelope(r#""task":{"task_id":"task-1234"}"#).replace("1.2.3", "1.2.3-rc1");
    assert_eq!(
        Document::parse(prerelease.as_bytes(), expected(), ParseLimits::default()).err(),
        Some(DecodeError::Version)
    );
    let compatible_patch =
        envelope(r#""task":{"task_id":"task-1234"}"#).replace("1.2.3", "1.2.99+build.7");
    assert!(
        Document::parse(
            compatible_patch.as_bytes(),
            expected(),
            ParseLimits::default()
        )
        .is_ok()
    );
}

#[test]
fn parser_byte_limit_is_checked_before_json_decoding() {
    let bytes = envelope(r#""task":{"task_id":"task-1234"}"#);
    let cap = bytes.len() - 1;
    assert_eq!(
        Document::parse(bytes.as_bytes(), expected(), ParseLimits { max_bytes: cap }).err(),
        Some(DecodeError::Size)
    );
}

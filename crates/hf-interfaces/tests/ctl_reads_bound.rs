#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Bounded headers of the page items: sizes by Python's rules, `sessions`, and the fallback.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::caller::PidObservation;
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::Value;

fn page(fake: &Fake, extra: &[&str]) -> Value {
    let mut list = vec!["task-list-page", "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["result"].clone()
}

fn settled() -> Fake {
    fake("completed", Some(0), PidObservation::Absent)
}

fn notes(width: usize, unit: &str) -> String {
    format!(r#","note_a":"{0}","note_b":"{0}""#, unit.repeat(width))
}

fn leaked(page: &Value, key: &str) -> usize {
    page["items"][0][key].as_str().unwrap().chars().count()
}

#[test]
fn sizes_follow_pythons_ensure_ascii_escapes() {
    // Two 200-character values: 404 bytes as ASCII, 2,404 as `\uXXXX` escapes. Only the
    // escaped form exceeds 2 KiB, so only it is cut again to 100 characters.
    let cases: [(&str, usize); 4] = [
        ("a", 200),
        ("\u{e9}", 100),
        (r"\u007f", 100),
        ("\u{4e2d}", 100),
    ];
    for (unit, kept) in cases {
        let mut fake = settled();
        fake.header_extra = Box::leak(notes(200, unit).into_boxed_str());
        let result = page(&fake, &[]);
        assert_eq!(leaked(&result, "note_a"), kept, "{unit:?}");
    }
    // An astral character costs 12 bytes (a surrogate pair): even 100 characters twice are
    // over 2 KiB, so the cut goes on to 32.
    let mut fake = settled();
    fake.header_extra = Box::leak(notes(200, "\u{1f600}").into_boxed_str());
    assert_eq!(leaked(&page(&fake, &[]), "note_a"), 32);
}

#[test]
fn the_cut_depends_on_python_size_not_on_utf8_length() {
    // 90 two-byte characters twice are 360 bytes in UTF-8 but 1,092 as Python writes them,
    // still under 2 KiB, so nothing is cut again.
    let mut fake = settled();
    fake.header_extra = Box::leak(notes(90, "\u{e9}").into_boxed_str());
    assert_eq!(leaked(&page(&fake, &[]), "note_a"), 90);
}

#[test]
fn sessions_are_normalized_to_the_orchestrator_cut_to_128() {
    let cases: [(&str, Value); 5] = [
        (
            r#","sessions":{"orchestrator":"o","other":1}"#,
            serde_json::json!({"orchestrator": "o"}),
        ),
        (r#","sessions":{"orchestrator":""}"#, serde_json::json!({})),
        (r#","sessions":{}"#, serde_json::json!({})),
        (r#","sessions":{"orchestrator":0}"#, serde_json::json!({})),
        (
            r#","sessions":{"orchestrator":12345}"#,
            serde_json::json!({"orchestrator": "12345"}),
        ),
    ];
    for (extra, wanted) in cases {
        let mut fake = settled();
        fake.header_extra = extra;
        let result = page(&fake, &[]);
        assert_eq!(result["items"][0]["sessions"], wanted, "{extra}");
    }
    let mut fake = settled();
    fake.header_extra = Box::leak(
        format!(r#","sessions":{{"orchestrator":"{}"}}"#, "o".repeat(300)).into_boxed_str(),
    );
    let result = page(&fake, &[]);
    assert_eq!(
        result["items"][0]["sessions"]["orchestrator"]
            .as_str()
            .unwrap()
            .len(),
        128
    );
    // Only listings are bounded; `status` keeps the stored value.
    fake.header_extra = r#","sessions":{"orchestrator":"o","other":1}"#;
    assert_eq!(task(&fake, &[])["sessions"]["other"], 1);
    fake.header_extra = r#","sessions":"text""#;
    assert_eq!(page(&fake, &[])["items"][0]["sessions"], "text");
}

fn heavy() -> &'static str {
    Box::leak(format!(r#","workflow_id":"{}""#, "w".repeat(1900)).into_boxed_str())
}

fn assert_incomplete(result: &Value) {
    assert_eq!(result["history_incomplete"], true);
    assert_eq!(result["counts_complete"], false);
    for key in [
        "total_active_count",
        "total_active_root_count",
        "total_attention_root_count",
    ] {
        assert!(result[key].is_null(), "{key}");
    }
}

#[test]
fn a_fallback_item_marks_history_and_counts_on_the_plain_page_and_explicit_ids() {
    let mut fake = settled();
    fake.header_extra = heavy();
    for extra in [vec![], vec!["--task-ids", ID]] {
        let result = page(&fake, &extra);
        assert_eq!(result["items"][0]["needs_direct_lookup"], true, "{extra:?}");
        assert_incomplete(&result);
    }
    let healthy = page(&settled(), &["--task-ids", ID]);
    assert!(
        healthy.get("history_incomplete").is_none(),
        "an id page carries no counts"
    );
}

#[test]
fn a_fallback_related_header_marks_history_and_counts_too() {
    let mut fake = settled();
    fake.related_extra = Some(heavy());
    for extra in [vec![], vec!["--task-ids", ID]] {
        let result = page(&fake, &extra);
        assert_eq!(result["items"][0].get("needs_direct_lookup"), None);
        assert_eq!(
            result["related_headers"][0]["needs_direct_lookup"], true,
            "{extra:?}"
        );
        assert_incomplete(&result);
    }
}

/// Python writes `str(True)` as `True`, not JSON's `true`.
#[test]
fn a_boolean_orchestrator_is_stringified_as_python_does() {
    let mut fake = settled();
    fake.header_extra = r#","sessions":{"orchestrator":true}"#;
    let result = page(&fake, &[]);
    assert_eq!(result["items"][0]["sessions"]["orchestrator"], "True");
}

//! Parity of JSON text, canonical bytes, digests and identity with the Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
#[path = "../../hf-application/tests/workflow_support/mod.rs"]
mod workflow_support;

use hf_application::workflow::identity::{definition_sha256, workflow_identity};
use hf_domain::workflow::json::Json;
use hf_interfaces::workflow::json_render::{Style, ascii_dumps_len, render};
use workflow_parity_support::{RealCodec, load_cases};

fn text<'a>(expected: &'a hf_domain::workflow::json::JsonObject, key: &str) -> &'a str {
    expected.get(key).and_then(Json::as_str).unwrap_or("")
}

#[test]
fn digest_cases_match_python() {
    let mut failures = Vec::new();
    for case in load_cases("digest.json") {
        let expected = case.expected();
        let value = case.input();
        let pairs = [
            (Style::Line, "line"),
            (Style::Pretty, "pretty"),
            (Style::Storage, "storage"),
            (Style::Canonical, "canonical"),
        ];
        for (style, key) in pairs {
            let got = render(value, style);
            if got != text(expected, key) {
                failures.push(format!("{}: {key} differs: {got}", case.id));
            }
        }
        if Some(ascii_dumps_len(value))
            != expected
                .get("ascii_len")
                .and_then(|n| n.as_int()?.to_i64())
                .and_then(|n| usize::try_from(n).ok())
        {
            failures.push(format!("{}: ascii_len differs", case.id));
        }
        if let Json::Object(definition) = value {
            if definition_sha256(&RealCodec, definition) != text(expected, "sha256") {
                failures.push(format!("{}: sha256 differs", case.id));
            }
            if workflow_identity(&RealCodec, definition) != text(expected, "identity") {
                failures.push(format!("{}: identity differs", case.id));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

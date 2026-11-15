//! Parity of the JSON reader with Python's `json.loads`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;

use hf_domain::workflow::json::Json;
use hf_interfaces::workflow::json_render::{Style, render};
use hf_interfaces::workflow::json_text::parse;
use workflow_parity_support::load_cases;

#[test]
fn reader_matches_json_loads() {
    let mut failures = Vec::new();
    for case in load_cases("json_text.json") {
        let expected = case.expected();
        let text = case.input().as_str().unwrap_or("");
        match (expected.get("ok"), parse(text)) {
            (Some(Json::Bool(true)), Ok(value)) => {
                if let Some(want) = expected.get("value") {
                    // NaN is unequal to itself; compare the written form.
                    if render(want, Style::Line) != render(&value, Style::Line) {
                        failures.push(format!("{}: value differs: {value:?}", case.id));
                    }
                }
            }
            (Some(Json::Bool(false)), Err(_)) => {}
            (_, outcome) => failures.push(format!("{}: outcome {outcome:?}", case.id)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

//! Parity of stored qualification with the Python `decode`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;

use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::Json;
use hf_interfaces::workflow::reason_message;
use workflow_parity_support::load_cases;

#[test]
fn decode_cases_match_python() {
    let mut failures = Vec::new();
    for case in load_cases("decode.json") {
        let expected = case.expected();
        let Json::Object(definition) = case.input() else {
            continue;
        };
        let source = case.get("source").as_str().unwrap_or("");
        match (expected.get("ok"), qualify_stored(definition, Some(source))) {
            (Some(Json::Bool(true)), Ok(())) => {}
            (Some(Json::Bool(false)), Err(reason)) => {
                let want = expected.get("reason").and_then(Json::as_str).unwrap_or("");
                if reason_message(&reason) != want {
                    failures.push(format!(
                        "{}: {:?} != {want:?}",
                        case.id,
                        reason_message(&reason)
                    ));
                }
            }
            (_, outcome) => failures.push(format!("{}: outcome {outcome:?}", case.id)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

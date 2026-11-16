//! Every definition the corpus validates also qualifies as a stored document, once the
//! store-owned fields are set as a save would set them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;

use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::DocumentReason;
use workflow_parity_support::load_cases;

fn as_saved(definition: &JsonObject) -> JsonObject {
    let mut saved = definition.clone();
    saved.insert(
        "workflow_id",
        Json::text("0123456789abcdef0123456789abcdef"),
    );
    saved.insert("revision", Json::int(1));
    saved.insert("updated_at", Json::Float(1.0));
    saved.insert("execution_contract", Json::text("delegation"));
    saved.insert("context_delivery", Json::text("optimized_v1"));
    saved
}

#[test]
fn qualify_accepts_every_valid_corpus_output() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for case in load_cases("validate.json") {
        let expected = case.expected();
        if expected.get("ok") != Some(&Json::Bool(true)) {
            continue;
        }
        let Some(Json::Object(definition)) = expected.get("definition") else {
            continue;
        };
        checked += 1;
        let saved = as_saved(definition);
        let name = definition.get("name").and_then(Json::as_str).unwrap_or("");
        if let Err(reason) = qualify_stored(&saved, Some(name)) {
            failures.push(format!("{}: {reason:?}", case.id));
        }
        if qualify_stored(&saved, None).is_err() {
            failures.push(format!("{}: refused without a source name", case.id));
        }
    }
    assert!(checked > 100, "only {checked} accepted cases");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn qualify_refuses_a_renamed_source_for_every_valid_output() {
    for case in load_cases("validate.json") {
        let expected = case.expected();
        let Some(Json::Object(definition)) = expected.get("definition") else {
            continue;
        };
        let result = qualify_stored(&as_saved(definition), Some("some other name"));
        assert_eq!(result, Err(DocumentReason::NameMismatch), "{}", case.id);
    }
}

//! The generated backend fact table of the test doubles agrees with the corpus.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
#[path = "../../hf-application/tests/workflow_support/mod.rs"]
mod workflow_support;

use hf_domain::workflow::json::Json;
use workflow_parity_support::read_fixture;
use workflow_support::backend_facts::{COMMANDS, FACTS};

fn flag(entry: &hf_domain::workflow::json::JsonObject, key: &str) -> bool {
    entry.get(key) == Some(&Json::Bool(true))
}

#[test]
fn backend_facts_table_matches_corpus() {
    let Json::Object(document) = read_fixture("backend_facts.json") else {
        panic!("not an object");
    };
    let Some(Json::Array(backends)) = document.get("backends") else {
        panic!("no backends");
    };
    assert_eq!(backends.len(), FACTS.len());
    for entry in backends {
        let entry = entry.as_object().unwrap();
        let name = entry.get("name").and_then(Json::as_str).unwrap();
        let row = FACTS.iter().find(|row| row.name == name).unwrap();
        assert_eq!(row.turn_cap, flag(entry, "turn_cap"), "{name} turn_cap");
        assert_eq!(
            row.model_selection,
            flag(entry, "model_selection"),
            "{name} model_selection"
        );
        assert_eq!(
            row.caller_approvals,
            flag(entry, "caller_approvals"),
            "{name} caller_approvals"
        );
        assert_eq!(
            row.workflow_caller_approvals,
            flag(entry, "workflow_caller_approvals"),
            "{name} workflow_caller_approvals"
        );
        let effort = entry.get("effort").and_then(Json::as_object).unwrap();
        assert_eq!(
            row.accepts_effort,
            flag(effort, "accepts_parameter"),
            "{name} accepts_effort"
        );
        let levels: Vec<&str> = effort
            .get("levels")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .filter_map(Json::as_str)
            .collect();
        assert_eq!(row.levels, levels.as_slice(), "{name} levels");
    }
}

#[test]
fn command_outcome_table_matches_corpus() {
    let Json::Object(document) = read_fixture("backend_facts.json") else {
        panic!("not an object");
    };
    let Some(Json::Array(outcomes)) = document.get("command_outcomes") else {
        panic!("no command outcomes");
    };
    assert_eq!(outcomes.len(), COMMANDS.len());
    for (entry, row) in outcomes.iter().zip(COMMANDS) {
        let entry = entry.as_object().unwrap();
        assert_eq!(
            entry.get("backend").and_then(Json::as_str),
            Some(row.backend)
        );
        assert_eq!(
            entry.get("freedom").and_then(Json::as_str),
            Some(row.freedom)
        );
        let outcome = entry.get("outcome").and_then(Json::as_object).unwrap();
        let message = outcome.get("message").and_then(Json::as_str);
        assert_eq!(row.refusal, message);
    }
}

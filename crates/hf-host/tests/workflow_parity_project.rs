//! Parity of the read projection of a stored definition with the Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;

use hf_application::workflow::validate::migrate::project_definition;
use hf_domain::workflow::json::Json;
use workflow_parity_support::{RealCodec, load_cases};

#[test]
fn projection_matches_python() {
    let mut failures = Vec::new();
    for case in load_cases("project.json") {
        let expected = case.expected();
        let Json::Object(definition) = case.input() else {
            continue;
        };
        match (
            expected.get("ok"),
            project_definition(&RealCodec, definition),
        ) {
            (Some(Json::Bool(true)), Ok(got)) => {
                if expected.get("definition") != Some(&Json::Object(got)) {
                    failures.push(format!("{}: definition differs", case.id));
                }
            }
            (Some(Json::Bool(false)), Err(_)) => {}
            _ => failures.push(format!("{}: outcome differs", case.id)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

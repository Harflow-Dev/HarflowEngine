//! Parity of definition validation with the Python reference, over the whole corpus.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
#[path = "../../hf-application/tests/workflow_support/mod.rs"]
mod workflow_support;

use hf_application::workflow::ports::DefinitionCodec;
use hf_application::workflow::validate::Validator;
use hf_application::workflow::validate::warnings::definition_warnings;
use hf_domain::workflow::json::Json;
use hf_interfaces::workflow::messages::failure_message;
use workflow_parity_support::{Case, RealCodec, load_cases, resolver_entries};
use workflow_support::{MapResolver, TableCatalog};

fn check(validator: &Validator<'_>, case: &Case) -> Option<String> {
    let expected = case.expected();
    let outcome = validator.validate(case.input());
    match (expected.get("ok"), outcome) {
        (Some(Json::Bool(true)), Ok(definition)) => {
            if let Some(Json::Str(digest)) = expected.get("definition_sha256") {
                // Large outputs are recorded by the digest of their canonical form.
                if RealCodec.definition_sha256(&definition) != *digest {
                    return Some(format!("{}: definition digest differs", case.id));
                }
            } else if expected.get("definition") != Some(&Json::Object(definition.clone())) {
                return Some(format!("{}: definition differs", case.id));
            }
            let warnings = Json::texts(&definition_warnings(&definition));
            (expected.get("warnings") != Some(&warnings))
                .then(|| format!("{}: warnings differ", case.id))
        }
        (Some(Json::Bool(false)), Err(failure)) => {
            let message = failure_message(&failure);
            let want = expected.get("message").and_then(Json::as_str).unwrap_or("");
            (message != want).then(|| format!("{}: message {message:?} != {want:?}", case.id))
        }
        (Some(Json::Bool(true)), Err(failure)) => Some(format!(
            "{}: refused {:?}",
            case.id,
            failure_message(&failure)
        )),
        (_, Ok(_)) => Some(format!("{}: accepted, expected refusal", case.id)),
        _ => Some(format!("{}: unexpected outcome", case.id)),
    }
}

#[test]
fn validate_matches_corpus() {
    let resolver = MapResolver::new(resolver_entries());
    let validator = Validator::new(&TableCatalog, &resolver);
    let cases = load_cases("validate.json");
    assert!(cases.len() > 100);
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|case| check(&validator, case))
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

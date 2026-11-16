//! The container depth bound on values built in code: validation and qualification refuse
//! them before anything recurses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_application::workflow::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::{DocumentReason, Rule};
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_interfaces::workflow::messages::failure_message;

const MAX: usize = 64;

/// `levels` arrays around a null, built without recursion.
fn nested(levels: usize) -> Json {
    let mut value = Json::Null;
    for _ in 0..levels {
        value = Json::Array(vec![value]);
    }
    value
}

/// Takes nested arrays apart so that dropping them does not recurse.
fn dismantle(value: Json) {
    let mut current = value;
    while let Json::Array(mut items) = current {
        current = items.pop().unwrap_or(Json::Null);
    }
}

/// The definition object holds `note`, so the whole value is `1 + levels` deep.
fn with_note(levels: usize) -> JsonObject {
    let mut definition = accepted(&top(""));
    definition.insert("note", nested(levels));
    definition
}

fn dispose(mut definition: JsonObject) {
    if let Some(note) = definition.remove("note") {
        dismantle(note);
    }
}

fn validate(definition: &JsonObject) -> Result<JsonObject, WorkflowFailure> {
    let resolver = resolver();
    Validator::new(&TableCatalog, &resolver).validate(&Json::Object(definition.clone()))
}

#[test]
fn validate_depth_64_passes_and_65_is_refused() {
    let at_limit = with_note(MAX - 1);
    let result = validate(&at_limit);
    assert!(result.is_ok(), "{:?}", result.err());
    dispose(result.unwrap());
    dispose(at_limit);

    let over = with_note(MAX);
    let failure = validate(&over).unwrap_err();
    assert_eq!(
        failure,
        WorkflowFailure::Refused(hf_domain::workflow::refusal::Refusal::new(
            DefinitionRule::TooDeep
        ))
    );
    assert_eq!(
        failure_message(&failure),
        "Workflow definition nests deeper than 64 levels"
    );
    assert!(matches!(
        &failure,
        WorkflowFailure::Refused(refusal) if refusal.rule == Rule::Definition(DefinitionRule::TooDeep)
    ));
    dispose(over);
}

#[test]
fn qualify_depth_64_passes_and_65_is_refused() {
    let at_limit = with_note(MAX - 1);
    assert_eq!(qualify_stored(&at_limit, None), Ok(()));
    dispose(at_limit);
    let over = with_note(MAX);
    let reason = qualify_stored(&over, None).unwrap_err();
    assert_eq!(reason, DocumentReason::TooDeep);
    assert_eq!(
        hf_interfaces::workflow::reason_message(&reason),
        "Definition nests deeper than 64 levels"
    );
    dispose(over);
}

#[test]
fn depth_counts_every_container_kind() {
    let mut definition = accepted(&top(""));
    let mut inner = JsonObject::new();
    inner.insert("k", nested(MAX - 2));
    definition.insert("note", Json::Object(inner));
    assert!(validate(&definition).is_ok());
    let mut deeper = JsonObject::new();
    deeper.insert("k", nested(MAX - 1));
    definition.insert("note", Json::Object(deeper));
    assert!(matches!(
        validate(&definition),
        Err(WorkflowFailure::Refused(refusal)) if refusal.rule == Rule::Definition(DefinitionRule::TooDeep)
    ));
    if let Some(Json::Object(mut map)) = definition.remove("note")
        && let Some(value) = map.remove("k")
    {
        dismantle(value);
    }
}

#[test]
fn a_very_deep_value_is_refused_without_recursion() {
    let depth = 100_000;
    let mut definition = accepted(&top(""));
    definition.insert("note", nested(depth));
    let raw = Json::Object(definition);
    let resolver = resolver();
    let failure = Validator::new(&TableCatalog, &resolver)
        .validate(&raw)
        .unwrap_err();
    assert_eq!(
        failure_message(&failure),
        "Workflow definition nests deeper than 64 levels"
    );
    assert!(
        Validator::new(&TableCatalog, &resolver)
            .normalize(&raw)
            .is_err()
    );
    let Json::Object(mut map) = raw else {
        panic!("not an object");
    };
    assert_eq!(qualify_stored(&map, None), Err(DocumentReason::TooDeep));
    dismantle(map.remove("note").unwrap());
}

#[test]
fn a_deep_array_as_the_whole_definition_is_refused() {
    let resolver = resolver();
    let raw = nested(100_000);
    let failure = Validator::new(&TableCatalog, &resolver)
        .validate(&raw)
        .unwrap_err();
    assert_eq!(
        failure_message(&failure),
        "Workflow definition nests deeper than 64 levels"
    );
    dismantle(raw);
}

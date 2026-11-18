//! Orchestrator overrides are applied or refused, never silently dropped.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_save_support;

use hf_application::workflow::snapshot::{SnapshotFailure, launch_definition};
use hf_application::workflow::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::Refusal;
use hf_domain::workflow::rule_shape::DefinitionRule;
use workflow_save_support::definition;
use workflow_save_support::workflow_support::{MapResolver, TableCatalog};

fn overrides() -> JsonObject {
    JsonObject::from_entries(vec![("model".to_string(), Json::text("other"))])
}

fn launch(source: &JsonObject, given: Option<&JsonObject>) -> Result<JsonObject, SnapshotFailure> {
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    launch_definition(&Validator::new(&catalog, &resolver), source, given)
}

fn refused() -> SnapshotFailure {
    SnapshotFailure::Validation(WorkflowFailure::Refused(Refusal::new(
        DefinitionRule::OrchestratorNotObject,
    )))
}

#[test]
fn overrides_without_an_orchestrator_are_refused() {
    let source = definition("flow");
    assert!(source.get("orchestrator").is_none());
    assert_eq!(launch(&source, Some(&overrides())), Err(refused()));
}

#[test]
fn overrides_over_a_non_object_orchestrator_are_refused() {
    let mut source = definition("flow");
    source.insert("orchestrator", Json::text("codex"));
    assert_eq!(launch(&source, Some(&overrides())), Err(refused()));
}

#[test]
fn empty_or_absent_overrides_launch_without_an_orchestrator() {
    let source = definition("flow");
    let empty = JsonObject::new();
    for given in [None, Some(&empty)] {
        let launched = launch(&source, given).unwrap();
        assert_eq!(
            launched.get("context_delivery"),
            Some(&Json::text("optimized_v1"))
        );
    }
}

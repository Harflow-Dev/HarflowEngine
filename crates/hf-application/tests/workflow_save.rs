//! Save orchestration: optimistic identity/revision checks, admission and final qualification.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_save_support;

use hf_application::workflow::catalog::CatalogFailure;
use hf_application::workflow::ports::DefinitionStore;
use hf_application::workflow::save::SaveExpectations;
use hf_application::workflow::validate::WorkflowFailure;
use hf_domain::workflow::json::{Json, JsonInt};
use hf_domain::workflow::refusal::{DocumentReason, Rule};
use hf_domain::workflow::rule_shape::OnDemandRule;
use workflow_save_support::{
    Allow, Deny, MemStore, SeqIds, StoreOperation, definition, reference_definition, save,
};

fn saved(store: &MemStore, ids: &SeqIds) -> Json {
    Json::Object(
        save(
            store,
            &definition("flow"),
            &SaveExpectations::default(),
            &Allow,
            ids,
        )
        .unwrap(),
    )
}

fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value
        .as_object()
        .unwrap()
        .get(key)
        .and_then(Json::as_str)
        .unwrap()
}

fn resource_definition(name: &str, mode: Option<&str>) -> hf_domain::workflow::json::JsonObject {
    let mut raw = definition(name);
    let nodes = raw.get_mut("nodes").unwrap().as_array_mut().unwrap();
    let agent = nodes[1].as_object_mut().unwrap();
    agent.insert("resources", Json::texts(&["database".to_string()]));
    if let Some(mode) = mode {
        agent.insert("execution_mode", Json::text(mode));
    }
    raw
}

#[test]
fn save_checks_identity_before_revision() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let existing = saved(&store, &ids);
    let error = save(
        &store,
        &definition("flow"),
        &SaveExpectations {
            workflow_id: Some("stale-id".to_string()),
            revision: None,
        },
        &Allow,
        &ids,
    )
    .unwrap_err();
    assert_eq!(
        error,
        CatalogFailure::StaleIdentity {
            expected: "stale-id".to_string(),
            current: Some(text(&existing, "workflow_id").to_string()),
        }
    );
    assert_eq!(
        store.operations().last(),
        Some(&StoreOperation::Begin {
            name: "flow".to_string(),
        })
    );
}

#[test]
fn existing_save_requires_an_expected_revision() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    saved(&store, &ids);
    assert_eq!(
        save(
            &store,
            &definition("flow"),
            &SaveExpectations {
                workflow_id: Some("00000000000000000000000000000001".to_string()),
                revision: None,
            },
            &Allow,
            &ids,
        )
        .unwrap_err(),
        CatalogFailure::StaleRevision {
            expected: None,
            current: JsonInt::from_i64(1),
        }
    );
}

#[test]
fn revision_above_i64_max_increments() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let mut current = save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Allow,
        &ids,
    )
    .unwrap();
    let revision = JsonInt::parse(false, "9223372036854775808").unwrap();
    current.insert("revision", Json::Int(revision.clone()));
    store.insert("flow", current);
    let result = save(
        &store,
        &definition("flow"),
        &SaveExpectations {
            workflow_id: Some("00000000000000000000000000000001".to_string()),
            revision: Some(revision),
        },
        &Allow,
        &ids,
    )
    .unwrap();
    assert_eq!(
        result
            .get("revision")
            .and_then(Json::as_int)
            .unwrap()
            .text(),
        "9223372036854775809"
    );
}

#[test]
fn revision_at_digit_limit_refuses_before_write() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let mut current = save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Allow,
        &ids,
    )
    .unwrap();
    let revision = JsonInt::parse(false, &"9".repeat(4300)).unwrap();
    current.insert("revision", Json::Int(revision.clone()));
    store.insert("flow", current.clone());
    let before_calls = store.calls();
    let before_ops = store.operations().len();
    let error = save(
        &store,
        &definition("flow"),
        &SaveExpectations {
            workflow_id: Some("00000000000000000000000000000001".to_string()),
            revision: Some(revision),
        },
        &Allow,
        &ids,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CatalogFailure::Validation(WorkflowFailure::Qualification(
            DocumentReason::IntegerTooLong { digits: 4301 }
        ))
    ));
    assert_eq!(store.calls(), before_calls);
    assert_eq!(
        store.get("flow").unwrap().unwrap().definition,
        current,
        "the stored value must survive a refusal"
    );
    assert_eq!(
        store.operations()[before_ops],
        StoreOperation::Begin {
            name: "flow".to_string()
        }
    );
}

#[test]
fn final_saved_document_contains_forced_fields_and_dependency_pin() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let child = save(
        &store,
        &definition("child"),
        &SaveExpectations::default(),
        &Allow,
        &ids,
    )
    .unwrap();
    let child_id = text(&Json::Object(child), "workflow_id").to_string();
    let root = reference_definition("root", &child_id);
    let final_document = save(&store, &root, &SaveExpectations::default(), &Allow, &ids).unwrap();
    assert_eq!(
        final_document.get("execution_contract"),
        Some(&Json::text("delegation"))
    );
    assert_eq!(
        final_document.get("context_delivery"),
        Some(&Json::text("optimized_v1"))
    );
    assert_eq!(final_document.get("revision"), Some(&Json::int(1)));
    assert_eq!(final_document.get("updated_at"), Some(&Json::Float(1234.5)));
    let pin = final_document
        .get("nodes")
        .and_then(Json::as_array)
        .unwrap()[1]
        .as_object()
        .unwrap();
    assert_eq!(pin.get("workflow_name"), Some(&Json::text("child")));
    assert_eq!(
        hf_domain::workflow::document::qualify_stored(&final_document, Some("root")),
        Ok(())
    );
}

#[test]
fn authority_refusal_does_not_replace_the_definition() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let error = save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Deny("caller lacks workflow authority"),
        &ids,
    )
    .unwrap_err();
    assert_eq!(
        error,
        CatalogFailure::Authority(hf_application::workflow::ports::AuthorityRefusal {
            message: "caller lacks workflow authority".to_string(),
        })
    );
    assert!(store.calls().is_empty());
    assert_eq!(
        store.operations(),
        [
            StoreOperation::Begin {
                name: "flow".to_string()
            },
            StoreOperation::List
        ]
    );
}

#[test]
fn invalid_final_candidate_is_refused_without_any_write() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let mut raw = definition("flow");
    raw.insert("routing_mode", Json::Null);
    let error = save(&store, &raw, &SaveExpectations::default(), &Allow, &ids).unwrap_err();
    assert!(matches!(error, CatalogFailure::Validation(_)));
    assert!(store.calls().is_empty());
    assert!(store.operations().is_empty());
}

#[test]
fn new_resource_node_defaults_headless() {
    let store = MemStore::new();
    let result = save(
        &store,
        &resource_definition("flow", None),
        &SaveExpectations::default(),
        &Allow,
        &SeqIds::default(),
    )
    .unwrap();
    let agent = result.get("nodes").and_then(Json::as_array).unwrap()[1]
        .as_object()
        .unwrap();
    assert_eq!(agent.get("execution_mode"), Some(&Json::text("headless")));
    assert_eq!(
        agent.get("resources"),
        Some(&Json::texts(&["database".to_string()]))
    );
}

#[test]
fn existing_prefer_subagent_node_adding_resources_is_refused() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Allow,
        &ids,
    )
    .unwrap();
    let mut first = definition("flow");
    first.get_mut("nodes").unwrap().as_array_mut().unwrap()[1]
        .as_object_mut()
        .unwrap()
        .insert("execution_mode", Json::text("prefer_subagent"));
    let old = save(
        &store,
        &first,
        &SaveExpectations {
            workflow_id: Some("00000000000000000000000000000001".to_string()),
            revision: Some(JsonInt::from_i64(1)),
        },
        &Allow,
        &ids,
    )
    .unwrap();
    let error = save(
        &store,
        &resource_definition("flow", None),
        &SaveExpectations {
            workflow_id: Some(text(&Json::Object(old.clone()), "workflow_id").to_string()),
            revision: old.get("revision").and_then(Json::as_int).cloned(),
        },
        &Allow,
        &ids,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CatalogFailure::Validation(WorkflowFailure::Refused(ref refusal))
            if refusal.rule == Rule::OnDemand(OnDemandRule::ResourcePreferSubagent)
    ));
    assert_eq!(
        store.calls().len(),
        2,
        "only the setup saves replaced the definition"
    );
    assert_eq!(store.get("flow").unwrap().unwrap().definition, old);
}

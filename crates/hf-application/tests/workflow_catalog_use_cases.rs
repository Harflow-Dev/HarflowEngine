//! Workflow get/list/delete use cases, including their store-wide failure boundaries.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_save_support;

use hf_application::workflow::catalog::{
    CatalogFailure, delete_definition, get_definition, list_definitions,
};
use hf_application::workflow::identity::workflow_identity;
use hf_application::workflow::ports::DefinitionStore;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::DocumentReason;
use workflow_save_support::{
    Allow, FakeCodec, MemStore, SeqIds, StoreCall, StoreOperation, definition, refusal, save,
};

fn saved(store: &MemStore, name: &str, ids: &SeqIds) -> JsonObject {
    save(store, &definition(name), &Default::default(), &Allow, ids).unwrap()
}

fn set_reference(definition: &mut JsonObject, target: &str) {
    let nodes = definition.get_mut("nodes").unwrap().as_array_mut().unwrap();
    let node = nodes[1].as_object_mut().unwrap();
    node.insert("type", Json::text("workflow"));
    node.insert(
        "workflow_ref",
        Json::Object(JsonObject::from_entries(vec![(
            "workflow_id".to_string(),
            Json::text(target),
        )])),
    );
}

#[test]
fn get_ignores_unrelated_corrupt_definition() {
    let store = MemStore::new();
    let current = saved(&store, "good", &SeqIds::default());
    store.insert_corrupt("broken", DocumentReason::NonFiniteValue);
    let prior_operations = store.operations().len();
    let projected = get_definition(&store, &FakeCodec, "good").unwrap();
    assert_eq!(projected.get("workflow_id"), current.get("workflow_id"));
    assert_eq!(
        &store.operations()[prior_operations..],
        [StoreOperation::Get {
            name: "good".to_string()
        }],
        "get must touch only its requested source"
    );
}

#[test]
fn list_and_save_refuse_corrupt_catalog_without_replacing() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let current = saved(&store, "good", &ids);
    store.insert_corrupt("broken", DocumentReason::NonFiniteValue);
    assert!(matches!(
        list_definitions(&store, &FakeCodec),
        Err(CatalogFailure::Document(_))
    ));
    assert!(matches!(
        save(
            &store,
            &definition("good"),
            &hf_application::workflow::save::SaveExpectations {
                workflow_id: current
                    .get("workflow_id")
                    .and_then(Json::as_str)
                    .map(str::to_string),
                revision: current.get("revision").and_then(Json::as_int).cloned(),
            },
            &Allow,
            &ids,
        ),
        Err(CatalogFailure::Document(_))
    ));
    assert!(
        matches!(store.calls().as_slice(), [StoreCall::Replace { name, .. }] if name == "good")
    );
    assert_eq!(
        &store.operations()[3..],
        [
            StoreOperation::List,
            StoreOperation::Begin {
                name: "good".to_string()
            },
            StoreOperation::List
        ]
    );
    assert_eq!(
        store.get("good").unwrap().unwrap().definition,
        current,
        "corrupt catalog entry must stop save before replace"
    );
    assert_eq!(
        store.calls().len(),
        1,
        "only the initial setup save replaced a definition"
    );
}

#[test]
fn delete_removes_referenced_definition_and_returns_sorted_notice_without_self() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let target = saved(&store, "target", &ids);
    let target_id = workflow_identity(&FakeCodec, &target);

    let mut self_reference = target.clone();
    set_reference(&mut self_reference, &target_id);
    store.insert("target", self_reference);
    for name in ["zeta", "alpha"] {
        let mut referenced = saved(&store, name, &ids);
        set_reference(&mut referenced, &target_id);
        store.insert(name, referenced);
    }

    let result = delete_definition(&store, &FakeCodec, "target").unwrap();
    assert_eq!(result.deleted, "target");
    assert_eq!(result.referenced_by, ["alpha", "zeta"]);
    assert_eq!(
        store.calls().last(),
        Some(&StoreCall::Remove {
            name: "target".to_string()
        })
    );
    assert_eq!(store.get("target").unwrap(), None);
}

#[test]
fn store_remove_failure_keeps_referenced_source() {
    let store = MemStore::new();
    let target = saved(&store, "target", &SeqIds::default());
    store.fail_next(
        StoreOperation::Remove {
            name: "target".to_string(),
        },
        refusal("target"),
    );
    let before = target.clone();
    assert!(delete_definition(&store, &FakeCodec, "target").is_err());
    assert_eq!(store.get("target").unwrap().unwrap().definition, before);
    assert_eq!(store.calls().len(), 2, "setup replace and attempted remove");
}

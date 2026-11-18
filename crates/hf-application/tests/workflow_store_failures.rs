//! Store port failures preserve the transaction's ordering and mutation boundaries.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_save_support;

use hf_application::workflow::catalog::{CatalogFailure, delete_definition};
use hf_application::workflow::ports::DefinitionStore;
use hf_application::workflow::save::SaveExpectations;
use workflow_save_support::{
    Allow, FakeCodec, MemStore, SeqIds, StoreOperation, definition, refusal, save,
};

#[test]
fn list_failure_stops_save_before_any_write() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    store.fail_next(StoreOperation::List, refusal("flow"));
    assert!(
        save(
            &store,
            &definition("flow"),
            &SaveExpectations::default(),
            &Allow,
            &ids,
        )
        .is_err()
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
fn replace_failure_records_attempt_but_does_not_change_storage() {
    let store = MemStore::new();
    store.fail_next(
        StoreOperation::Replace {
            name: "flow".to_string(),
        },
        refusal("flow"),
    );
    assert!(
        save(
            &store,
            &definition("flow"),
            &SaveExpectations::default(),
            &Allow,
            &SeqIds::default(),
        )
        .is_err()
    );
    assert_eq!(store.calls().len(), 1, "the attempted write is recorded");
    assert_eq!(store.get("flow").unwrap(), None);
    assert_eq!(
        &store.operations()[..3],
        [
            StoreOperation::Begin {
                name: "flow".to_string()
            },
            StoreOperation::List,
            StoreOperation::Replace {
                name: "flow".to_string()
            }
        ]
    );
}

#[test]
fn begin_failure_stops_before_listing() {
    let store = MemStore::new();
    store.fail_next(
        StoreOperation::Begin {
            name: "flow".to_string(),
        },
        refusal("flow"),
    );
    assert!(
        save(
            &store,
            &definition("flow"),
            &SaveExpectations::default(),
            &Allow,
            &SeqIds::default(),
        )
        .is_err()
    );
    assert_eq!(
        store.operations(),
        [StoreOperation::Begin {
            name: "flow".to_string()
        }]
    );
    assert!(store.calls().is_empty());
}

#[test]
fn delete_use_case_remove_failure_keeps_source() {
    let store = MemStore::new();
    save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Allow,
        &SeqIds::default(),
    )
    .unwrap();
    store.fail_next(
        StoreOperation::Remove {
            name: "flow".to_string(),
        },
        refusal("flow"),
    );
    let result = delete_definition(&store, &FakeCodec, "flow");
    assert_eq!(
        result.unwrap_err(),
        CatalogFailure::Document(refusal("flow"))
    );
    assert!(store.get("flow").unwrap().is_some());
}

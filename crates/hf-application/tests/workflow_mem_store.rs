//! Compile-level proof of the workflow test doubles.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::ports::{DefinitionStore, DefinitionTxn};
use hf_domain::workflow::json::JsonObject;
use hf_domain::workflow::refusal::DocumentReason;
use workflow_support::MemStore;

#[test]
fn mem_store_filename_order() {
    let store = MemStore::new();
    store.insert("a", JsonObject::new());
    store.insert("a-b", JsonObject::new());
    let names: Vec<String> = store
        .list()
        .unwrap()
        .into_iter()
        .map(|item| item.name)
        .collect();
    assert_eq!(names, ["a-b", "a"]);
}

#[test]
fn corrupt_entry_blocks_list_but_not_get() {
    let store = MemStore::new();
    store.insert("good", JsonObject::new());
    store.insert_corrupt("bad", DocumentReason::NonFiniteValue);
    assert!(store.list().is_err());
    assert!(store.get("good").unwrap().is_some());
    let txn = store.begin("good").unwrap();
    txn.replace(&JsonObject::new()).unwrap();
    assert_eq!(store.calls().len(), 1);
}

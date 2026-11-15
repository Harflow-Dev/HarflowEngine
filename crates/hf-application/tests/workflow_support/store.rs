//! An in-memory `DefinitionStore` that lists in filename order and can hold corrupt entries.
use std::cell::RefCell;
use std::collections::BTreeMap;

use hf_application::workflow::ports::{DefinitionStore, DefinitionTxn, StoredDefinition};
use hf_domain::workflow::json::JsonObject;
use hf_domain::workflow::refusal::{DocumentCode, DocumentReason, DocumentRefusal};

enum Entry {
    Valid(StoredDefinition),
    Corrupt(DocumentRefusal),
}

/// What the store was asked to write.
#[derive(Clone, Debug, PartialEq)]
pub enum StoreCall {
    Replace {
        name: String,
        definition: JsonObject,
    },
    Remove {
        name: String,
    },
}

#[derive(Default)]
pub struct MemStore {
    entries: RefCell<BTreeMap<String, Entry>>,
    calls: RefCell<Vec<StoreCall>>,
}

fn filename(name: &str) -> String {
    format!("{name}.json")
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, name: &str, definition: JsonObject) {
        let stored = StoredDefinition {
            name: name.to_string(),
            definition,
        };
        self.entries
            .borrow_mut()
            .insert(filename(name), Entry::Valid(stored));
    }

    pub fn insert_corrupt(&self, name: &str, reason: DocumentReason) {
        let refusal = DocumentRefusal {
            name: name.to_string(),
            code: DocumentCode::Incompatible,
            reason,
        };
        self.entries
            .borrow_mut()
            .insert(filename(name), Entry::Corrupt(refusal));
    }

    /// The writes requested so far, oldest first.
    pub fn calls(&self) -> Vec<StoreCall> {
        self.calls.borrow().clone()
    }

    fn read(&self, name: &str) -> Result<Option<StoredDefinition>, DocumentRefusal> {
        match self.entries.borrow().get(&filename(name)) {
            None => Ok(None),
            Some(Entry::Valid(stored)) => Ok(Some(stored.clone())),
            Some(Entry::Corrupt(refusal)) => Err(refusal.clone()),
        }
    }

    fn read_all(&self) -> Result<Vec<StoredDefinition>, DocumentRefusal> {
        let mut found = Vec::new();
        for entry in self.entries.borrow().values() {
            match entry {
                Entry::Valid(stored) => found.push(stored.clone()),
                Entry::Corrupt(refusal) => return Err(refusal.clone()),
            }
        }
        Ok(found)
    }
}

pub struct MemTxn<'a> {
    store: &'a MemStore,
    name: String,
    current: Option<StoredDefinition>,
}

impl DefinitionStore for MemStore {
    type Txn<'a> = MemTxn<'a>;

    fn get(&self, name: &str) -> Result<Option<StoredDefinition>, DocumentRefusal> {
        self.read(name)
    }

    fn list(&self) -> Result<Vec<StoredDefinition>, DocumentRefusal> {
        self.read_all()
    }

    fn begin(&self, name: &str) -> Result<MemTxn<'_>, DocumentRefusal> {
        Ok(MemTxn {
            store: self,
            name: name.to_string(),
            current: self.read(name)?,
        })
    }
}

impl DefinitionTxn for MemTxn<'_> {
    fn current(&self) -> Option<&StoredDefinition> {
        self.current.as_ref()
    }

    fn list(&self) -> Result<Vec<StoredDefinition>, DocumentRefusal> {
        self.store.read_all()
    }

    fn replace(self, definition: &JsonObject) -> Result<(), DocumentRefusal> {
        self.store.calls.borrow_mut().push(StoreCall::Replace {
            name: self.name.clone(),
            definition: definition.clone(),
        });
        self.store.insert(&self.name, definition.clone());
        Ok(())
    }

    fn remove(self) -> Result<(), DocumentRefusal> {
        self.store.calls.borrow_mut().push(StoreCall::Remove {
            name: self.name.clone(),
        });
        self.store
            .entries
            .borrow_mut()
            .remove(&filename(&self.name));
        Ok(())
    }
}

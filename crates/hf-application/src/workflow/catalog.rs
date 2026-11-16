//! Pure catalog operations over caller-injected definition storage.
use super::identity::workflow_identity;
use super::ports::{
    AuthorityRefusal, DefinitionCodec, DefinitionStore, DefinitionTxn, StoredDefinition,
};
use super::references::ReferenceFailure;
use super::validate::{WorkflowFailure, migrate::project_definition};
use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::{JsonInt, JsonObject};
use hf_domain::workflow::refusal::{
    DependencyRefusal, DocumentCode, DocumentReason, DocumentRefusal, Refusal,
};
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_domain::workflow::tree::referencing_definitions;

#[derive(Clone, Debug, PartialEq)]
pub enum CatalogFailure {
    Validation(WorkflowFailure),
    Dependency(DependencyRefusal),
    Document(DocumentRefusal),
    Authority(AuthorityRefusal),
    Missing {
        name: String,
    },
    StaleIdentity {
        expected: String,
        current: Option<String>,
    },
    StaleRevision {
        expected: Option<JsonInt>,
        current: JsonInt,
    },
}

impl From<ReferenceFailure> for CatalogFailure {
    fn from(value: ReferenceFailure) -> Self {
        match value {
            ReferenceFailure::Validation(error) => Self::Validation(error),
            ReferenceFailure::Dependency(error) => Self::Dependency(error),
        }
    }
}

pub(crate) fn check_name(name: &str) -> Result<(), CatalogFailure> {
    if hf_domain::workflow::name::is_workflow_name(name) {
        return Ok(());
    }
    Err(CatalogFailure::Validation(WorkflowFailure::Refused(
        Refusal::new(DefinitionRule::InvalidWorkflowName),
    )))
}

pub(crate) fn document_error(name: &str, reason: DocumentReason) -> CatalogFailure {
    CatalogFailure::Document(DocumentRefusal {
        name: name.to_string(),
        code: DocumentCode::Incompatible,
        reason,
    })
}

pub(crate) fn qualify(source: &StoredDefinition) -> Result<(), CatalogFailure> {
    qualify_stored(&source.definition, Some(&source.name))
        .map_err(|reason| document_error(&source.name, reason))
}

pub(crate) fn qualify_named(
    source: &StoredDefinition,
    requested: &str,
) -> Result<(), CatalogFailure> {
    if source.name != requested {
        return Err(document_error(requested, DocumentReason::NameMismatch));
    }
    qualify(source)
}

pub(crate) fn project(
    codec: &dyn DefinitionCodec,
    source: &StoredDefinition,
) -> Result<JsonObject, CatalogFailure> {
    qualify(source)?;
    project_definition(codec, &source.definition)
        .map_err(|error| CatalogFailure::Validation(WorkflowFailure::Refused(error)))
}

pub(crate) fn project_all(
    codec: &dyn DefinitionCodec,
    sources: &[StoredDefinition],
) -> Result<Vec<JsonObject>, CatalogFailure> {
    sources
        .iter()
        .map(|source| project(codec, source))
        .collect()
}

/// Read only the named source; unrelated corrupt entries do not affect it.
pub fn get_definition(
    store: &impl DefinitionStore,
    codec: &dyn DefinitionCodec,
    name: &str,
) -> Result<JsonObject, CatalogFailure> {
    check_name(name)?;
    let source = store
        .get(name)
        .map_err(CatalogFailure::Document)?
        .ok_or_else(|| CatalogFailure::Missing {
            name: name.to_string(),
        })?;
    qualify_named(&source, name)?;
    project(codec, &source)
}

/// Project the whole catalog, preserving the port's filename order; any failure refuses all.
pub fn list_definitions(
    store: &impl DefinitionStore,
    codec: &dyn DefinitionCodec,
) -> Result<Vec<JsonObject>, CatalogFailure> {
    let sources = store.list().map_err(CatalogFailure::Document)?;
    project_all(codec, &sources)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteResult {
    pub deleted: String,
    /// Sorted direct references, excluding the deleted definition itself.
    pub referenced_by: Vec<String>,
}

/// References are notices, not a restriction on deletion.
pub fn delete_definition(
    store: &impl DefinitionStore,
    codec: &dyn DefinitionCodec,
    name: &str,
) -> Result<DeleteResult, CatalogFailure> {
    check_name(name)?;
    let txn = store.begin(name).map_err(CatalogFailure::Document)?;
    let current = txn.current().ok_or_else(|| CatalogFailure::Missing {
        name: name.to_string(),
    })?;
    qualify_named(current, name)?;
    let id = workflow_identity(codec, &current.definition);
    let sources = txn.list().map_err(CatalogFailure::Document)?;
    let definitions = project_all(codec, &sources)?;
    let referenced_by = referencing_definitions(&definitions, &id)
        .into_iter()
        .filter(|item| item != name)
        .collect();
    txn.remove().map_err(CatalogFailure::Document)?;
    Ok(DeleteResult {
        deleted: name.to_string(),
        referenced_by,
    })
}

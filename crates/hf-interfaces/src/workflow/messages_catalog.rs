//! Rendering the structured outcomes of pure definition catalog use cases.
use super::{
    messages::{failure_message, message},
    reason_message,
};
use hf_application::workflow::catalog::{CatalogFailure, DeleteResult};

pub fn catalog_failure_message(failure: &CatalogFailure) -> String {
    match failure {
        CatalogFailure::Validation(error) => failure_message(error),
        CatalogFailure::Dependency(error) => message(&error.refusal),
        CatalogFailure::Document(error) => {
            format!("{}: {}", error.name, reason_message(&error.reason))
        }
        CatalogFailure::Authority(error) => error.message.clone(),
        CatalogFailure::Missing { name } => format!("Workflow {name} not found"),
        CatalogFailure::StaleIdentity { expected, current } => format!(
            "Stale workflow identity: expected {expected}, current {}",
            current.as_deref().unwrap_or("None")
        ),
        CatalogFailure::StaleRevision { expected, current } => format!(
            "Stale workflow revision: expected {}, current {}",
            expected
                .as_ref()
                .map_or_else(|| "None".to_string(), |value| value.text()),
            current.text()
        ),
    }
}

/// A successful deletion's optional warning; references never prevent removal.
pub fn deletion_notice(result: &DeleteResult) -> Option<String> {
    if result.referenced_by.is_empty() {
        None
    } else {
        Some(format!(
            "This workflow is still referenced by: {}",
            result.referenced_by.join(", ")
        ))
    }
}

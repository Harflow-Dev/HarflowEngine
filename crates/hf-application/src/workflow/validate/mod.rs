//! `validate_definition`: normalizes a candidate definition and refuses what cannot be
//! saved. The checks run in Python's order, which also fixes the key order of the result.
use super::ports::{BackendCatalog, PathResolver};

mod candidate;
mod candidate_backend;
pub mod field;
pub mod migrate;

/// Validation over the backend catalog and path resolver of the host.
pub struct Validator<'a> {
    pub catalog: &'a dyn BackendCatalog,
    pub resolver: &'a dyn PathResolver,
}

impl<'a> Validator<'a> {
    pub fn new(catalog: &'a dyn BackendCatalog, resolver: &'a dyn PathResolver) -> Self {
        Self { catalog, resolver }
    }
}

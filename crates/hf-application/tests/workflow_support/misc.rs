//! Resolver, clock and id doubles.
use std::cell::Cell;
use std::collections::HashMap;

use hf_application::workflow::ports::{Clock, IdSource, PathResolver};

/// Resolves only the paths it was given; any other path is a refusal.
#[derive(Default)]
pub struct MapResolver {
    paths: HashMap<String, Result<String, String>>,
}

impl MapResolver {
    pub fn new(entries: impl IntoIterator<Item = (String, Result<String, String>)>) -> Self {
        Self {
            paths: entries.into_iter().collect(),
        }
    }

    /// A resolver that returns every path unchanged.
    pub fn identity(paths: &[&str]) -> Self {
        Self::new(
            paths
                .iter()
                .map(|path| (path.to_string(), Ok(path.to_string()))),
        )
    }
}

impl PathResolver for MapResolver {
    fn resolve_absolute(&self, path: &str) -> Result<String, String> {
        match self.paths.get(path) {
            Some(result) => result.clone(),
            None => Err(format!("unmapped path {path}")),
        }
    }
}

pub struct FixedClock(pub f64);

impl Clock for FixedClock {
    fn unix_seconds(&self) -> f64 {
        self.0
    }
}

/// Ids `0000...0001`, `0000...0002`, and so on.
#[derive(Default)]
pub struct SeqIds {
    next: Cell<u128>,
}

impl IdSource for SeqIds {
    fn workflow_id(&self) -> String {
        let value = self.next.get() + 1;
        self.next.set(value);
        format!("{value:032x}")
    }
}

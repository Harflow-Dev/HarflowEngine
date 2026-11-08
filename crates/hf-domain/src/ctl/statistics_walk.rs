//! The ancestry walk of the statistics projection: each task's chain of continuations,
//! memoized when it ends normally, with the issues the chain meets.
use super::statistics::{Diagnostic, Expired, StatFacts};
use crate::task_read::TaskId;
use std::collections::{BTreeMap, BTreeSet};

/// Ancestors followed before the walk gives up.
const DEPTH_LIMIT: usize = 128;

pub(super) struct Walk<'a> {
    pub(super) tasks: BTreeMap<&'a str, &'a StatFacts>,
    pub(super) resolved: BTreeSet<String>,
    pub(super) unknown: BTreeSet<String>,
    pub(super) ancestry: BTreeMap<String, BTreeSet<String>>,
    pub(super) issues: Vec<(Vec<String>, Diagnostic)>,
    pub(super) expired: &'a dyn Fn() -> bool,
}

impl Walk<'_> {
    fn issue(&mut self, code: &str, message: &str, scope: Vec<String>) {
        self.issues.push((scope, Diagnostic::new(code, message)));
    }

    /// The ancestry of one task, memoized when the walk ends normally. The depth, cycle and
    /// missing-parent outcomes are not memoized, so each caller meets them again.
    pub(super) fn owner(&mut self, id: &str, mut seen: BTreeSet<String>) -> Result<(), Expired> {
        if (self.expired)() {
            return Err(Expired);
        }
        if self.resolved.contains(id) {
            return Ok(());
        }
        if seen.len() >= DEPTH_LIMIT {
            self.unknown.insert(id.to_string());
            let mut scope: BTreeSet<String> = seen;
            scope.insert(id.to_string());
            let message = "Task ancestry depth exceeds bounded projection";
            self.issue(
                "ownership_unavailable",
                message,
                scope.into_iter().collect(),
            );
            return Ok(());
        }
        if seen.contains(id) {
            self.unknown.insert(id.to_string());
            let scope: Vec<String> = seen.into_iter().collect();
            self.issue("ownership_ambiguous", "Task ancestry cycle", scope);
            return Ok(());
        }
        seen.insert(id.to_string());
        let Some(task) = self.tasks.get(id).copied() else {
            self.unknown.insert(id.to_string());
            let message = "Task ancestor metadata is unavailable";
            self.issue("ownership_unavailable", message, seen.into_iter().collect());
            return Ok(());
        };
        let mut linked: BTreeSet<String> = BTreeSet::new();
        let parent = task.continuation_of.as_ref().map(TaskId::as_str);
        if let Some(parent) = parent.filter(|parent| !parent.is_empty()) {
            if parent == id {
                self.unknown.insert(id.to_string());
                let message = "Task is its own ancestor";
                self.issue("ownership_ambiguous", message, [id.to_string()].to_vec());
            } else {
                linked.insert(parent.to_string());
                self.owner(parent, seen.clone())?;
                if self.unknown.contains(parent) {
                    self.unknown.insert(id.to_string());
                }
                if let Some(above) = self.ancestry.get(parent) {
                    linked.extend(above.iter().cloned());
                }
            }
        }
        self.resolved.insert(id.to_string());
        self.ancestry.insert(id.to_string(), linked);
        Ok(())
    }
}

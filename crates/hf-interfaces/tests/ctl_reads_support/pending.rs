//! The pending launch queries behind the `Fake`: launch rows in receipt filename order, a
//! scripted namespace and binding, and a scripted answer of the check of ordinary records.
use super::Fake;
use hf_application::ctl::task_reads::{
    Namespace, NamespaceFailure, PendingBinding, PendingBudget, PendingLimits, PendingPort,
    StoreFailure,
};
use hf_domain::{
    ctl::pending::{LaunchFacts, SUFFIX},
    task_read::TaskId,
};

pub struct PendingScript {
    pub checked: bool,
    pub binding: PendingBinding,
    pub namespace: Result<Namespace, NamespaceFailure>,
    pub rows: Vec<LaunchFacts>,
    /// The limits the services report.
    pub limits: PendingLimits,
}

impl Default for PendingScript {
    fn default() -> Self {
        Self {
            checked: true,
            binding: PendingBinding {
                root: "f".repeat(64),
                identity: [1, 2, 1, 3],
            },
            namespace: Ok(Namespace {
                revision: "a".repeat(64),
                invalid: 0,
            }),
            rows: Vec::new(),
            limits: PendingLimits::default(),
        }
    }
}

fn key(id: &str) -> String {
    format!("{id}{SUFFIX}")
}

impl PendingPort for Fake {
    fn pending_binding(&self, _root: &str) -> Result<PendingBinding, StoreFailure> {
        Ok(self.pending.binding.clone())
    }
    fn candidates_checked(
        &self,
        _budget: &PendingBudget,
        _limits: &PendingLimits,
    ) -> Result<bool, StoreFailure> {
        Ok(self.pending.checked)
    }
    fn pending_namespace(&self, _limits: &PendingLimits) -> Result<Namespace, NamespaceFailure> {
        self.pending.namespace.clone()
    }
    fn launch_name_exists(&self, id: &str) -> Result<bool, StoreFailure> {
        Ok(self.pending.rows.iter().any(|facts| facts.id == id))
    }
    fn next_launch_name(&self, after: Option<&str>) -> Result<Option<String>, StoreFailure> {
        let after = after.map(key);
        Ok(self
            .pending
            .rows
            .iter()
            .map(|facts| facts.id.clone())
            .filter(|id| after.as_ref().is_none_or(|after| &key(id) > after))
            .min_by_key(|id| key(id)))
    }
    fn record_size(&self, id: &TaskId) -> Result<Option<u64>, StoreFailure> {
        let recorded = self
            .pending
            .rows
            .iter()
            .any(|facts| facts.id == id.as_str() && facts.record);
        Ok(recorded.then_some(0))
    }
    fn pending_row(&self, id: &TaskId) -> Result<Option<LaunchFacts>, StoreFailure> {
        Ok(self
            .pending
            .rows
            .iter()
            .find(|facts| facts.id == id.as_str())
            .cloned())
    }
}

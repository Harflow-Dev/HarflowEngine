//! The pending launch queries of the scripted world: launch rows in filename order, a
//! scripted caller check, namespace and binding, and a record of what was asked.
use super::World;
use hf_application::ctl::task_reads::{
    Namespace, NamespaceFailure, PendingBinding, PendingBudget, PendingLimits, PendingPort,
    StoreFailure,
};
use hf_domain::{
    ctl::pending::{LaunchFacts, SUFFIX},
    task_read::TaskId,
};
use std::cell::RefCell;

pub struct PendingWorld {
    /// The answer of the check of every ordinary record.
    pub checked: Result<bool, StoreFailure>,
    /// Bytes the check charges to the budget.
    pub charge: u64,
    pub binding: Result<PendingBinding, StoreFailure>,
    pub namespace: Result<Namespace, NamespaceFailure>,
    /// The launch rows; the world serves them in receipt filename order.
    pub rows: Vec<LaunchFacts>,
    pub rows_failure: Option<StoreFailure>,
    /// The stored size every ordinary record reports.
    pub record_bytes: u64,
    /// Every `next_launch_name` question: the position and the count.
    pub asked: RefCell<Vec<(Option<String>, usize)>>,
}

impl Default for PendingWorld {
    fn default() -> Self {
        Self {
            checked: Ok(true),
            charge: 0,
            binding: Ok(PendingBinding {
                root: "r".repeat(64),
                identity: [1, 2, 3, 4],
            }),
            namespace: Ok(Namespace {
                revision: "n".repeat(64),
                invalid: 0,
            }),
            rows: Vec::new(),
            rows_failure: None,
            record_bytes: 0,
            asked: RefCell::new(Vec::new()),
        }
    }
}

fn key(id: &str) -> String {
    format!("{id}{SUFFIX}")
}

impl World {
    fn ordered_launches(&self) -> Vec<LaunchFacts> {
        let mut rows = self.pending_world.rows.clone();
        rows.sort_by_key(|facts| key(&facts.id));
        rows
    }
}

impl PendingPort for World {
    fn pending_binding(&self, _root: &str) -> Result<PendingBinding, StoreFailure> {
        self.note("pending_binding".into());
        self.pending_world.binding.clone()
    }
    fn candidates_checked(
        &self,
        budget: &PendingBudget,
        _limits: &PendingLimits,
    ) -> Result<bool, StoreFailure> {
        self.note("candidates_checked".into());
        let checked = self.pending_world.checked.clone()?;
        Ok(checked && budget.charge(self.pending_world.charge))
    }
    fn pending_namespace(&self, _limits: &PendingLimits) -> Result<Namespace, NamespaceFailure> {
        self.note("pending_namespace".into());
        self.pending_world.namespace.clone()
    }
    fn launch_name_exists(&self, id: &str) -> Result<bool, StoreFailure> {
        self.note("launch_name_exists".into());
        Ok(self.pending_world.rows.iter().any(|facts| facts.id == id))
    }
    fn next_launch_name(&self, after: Option<&str>) -> Result<Option<String>, StoreFailure> {
        self.note("next_launch_name".into());
        self.pending_world
            .asked
            .borrow_mut()
            .push((after.map(str::to_owned), 1));
        if let Some(failure) = &self.pending_world.rows_failure {
            return Err(failure.clone());
        }
        let after = after.map(key);
        Ok(self
            .ordered_launches()
            .into_iter()
            .find(|facts| after.as_ref().is_none_or(|after| &key(&facts.id) > after))
            .map(|facts| facts.id))
    }
    fn record_size(&self, id: &TaskId) -> Result<Option<u64>, StoreFailure> {
        self.note(format!("record_size:{}", id.as_str()));
        let recorded = self
            .pending_world
            .rows
            .iter()
            .any(|facts| facts.id == id.as_str() && facts.record);
        Ok(recorded.then_some(self.pending_world.record_bytes))
    }
    fn pending_row(&self, id: &TaskId) -> Result<Option<LaunchFacts>, StoreFailure> {
        self.note(format!("pending_row:{}", id.as_str()));
        Ok(self
            .pending_world
            .rows
            .iter()
            .find(|facts| facts.id == id.as_str())
            .cloned())
    }
}

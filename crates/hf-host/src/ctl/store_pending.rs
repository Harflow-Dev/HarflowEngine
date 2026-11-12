//! The pending launch queries of the read-only snapshot.
use super::store_reads::{Snapshot, read};
use hf_application::ctl::task_reads::{
    Namespace, NamespaceFailure, PendingBinding, PendingBudget, PendingLimits, PendingPort,
    StoreFailure,
};
use hf_domain::{ctl::pending::LaunchFacts, task_read::TaskId};

impl PendingPort for Snapshot<'_, '_> {
    fn pending_binding(&self, root: &str) -> Result<PendingBinding, StoreFailure> {
        read(self.0.pending_binding(root))
    }
    fn candidates_checked(
        &self,
        budget: &PendingBudget,
        limits: &PendingLimits,
    ) -> Result<bool, StoreFailure> {
        read(self.0.candidates_checked(budget, limits))
    }
    fn pending_namespace(&self, limits: &PendingLimits) -> Result<Namespace, NamespaceFailure> {
        match read(self.0.pending_namespace(limits))? {
            Some(namespace) => Ok(namespace),
            None => Err(NamespaceFailure::Bound),
        }
    }
    fn launch_name_exists(&self, id: &str) -> Result<bool, StoreFailure> {
        read(self.0.launch_name_exists(id))
    }
    fn next_launch_name(&self, after: Option<&str>) -> Result<Option<String>, StoreFailure> {
        read(self.0.next_launch_name(after))
    }
    fn record_size(&self, id: &TaskId) -> Result<Option<u64>, StoreFailure> {
        read(self.0.record_size(id))
    }
    fn pending_row(&self, id: &TaskId) -> Result<Option<LaunchFacts>, StoreFailure> {
        read(self.0.pending_row(id))
    }
}

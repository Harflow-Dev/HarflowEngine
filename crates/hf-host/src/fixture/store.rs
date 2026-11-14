//! Dispatcher-only per-command storage adapter.
use super::*;
use hf_domain::ownership::*;
use hf_storage::{Limits, Store};
pub(super) struct Commands<'a> {
    root: &'a str,
    until: Instant,
}
impl<'a> Commands<'a> {
    pub(super) fn new(root: &'a str, until: Instant) -> Self {
        Self { root, until }
    }
    fn open(&self) -> io::Result<Store> {
        map(Store::open_with(
            self.root,
            Limits::default(),
            self.until,
            hf_platform::lease::shared(),
            crate::durability::suppressed(),
        ))
    }
    pub(super) fn prepare(&self, plan: &ParentPlan) -> io::Result<PrepareResult> {
        let store = if plan.initialize {
            map(Store::initialize_fixture_with(
                self.root,
                Limits::default(),
                self.until,
                hf_platform::lease::shared(),
                crate::durability::suppressed(),
            ))?
        } else {
            self.open()?
        };
        let mut request = plan.request.clone();
        if plan.initialize {
            request.generation = map(Id::new(store.identity().generation.clone()))?;
        }
        map(store.ownership_prepare(&request, self.until))
    }
    pub(super) fn initialize(&self) -> io::Result<Id> {
        let store = map(Store::initialize_fixture_with(
            self.root,
            Limits::default(),
            self.until,
            hf_platform::lease::shared(),
            crate::durability::suppressed(),
        ))?;
        map(Id::new(store.identity().generation.clone()))
    }
    pub(super) fn observe(&self, operation: &Id) -> io::Result<Snapshot> {
        map(self.open()?.ownership_observe(operation, self.until))?.ok_or_else(invalid)
    }
}

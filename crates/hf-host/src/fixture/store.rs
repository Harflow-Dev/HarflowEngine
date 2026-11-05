//! Per-command storage adapter: no connection or lock handle escapes a method.
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
        map(Store::open(
            self.root,
            Limits::default(),
            self.until,
            hf_platform::lease::shared(),
        ))
    }
    pub(super) fn prepare(&self, plan: &ParentPlan) -> io::Result<PrepareResult> {
        let store = if plan.initialize {
            map(Store::initialize_fixture(
                self.root,
                Limits::default(),
                self.until,
                hf_platform::lease::shared(),
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
        let store = map(Store::initialize_fixture(
            self.root,
            Limits::default(),
            self.until,
            hf_platform::lease::shared(),
        ))?;
        map(Id::new(store.identity().generation.clone()))
    }
    pub(super) fn observe(&self, operation: &Id) -> io::Result<Snapshot> {
        map(self.open()?.ownership_observe(operation, self.until))?.ok_or_else(invalid)
    }
    pub(super) fn claim(&self, invitation: &Invitation) -> io::Result<ClaimResult> {
        let owner = OwnerClaim {
            owner: invitation.owner.clone(),
            nonce: invitation.nonce.clone(),
            process: invitation.process.clone(),
        };
        map(self.open()?.ownership_claim(
            &invitation.operation,
            &invitation.generation,
            invitation.revision,
            &owner,
            self.until,
        ))
    }
    pub(super) fn observed(&self, fence: &Fence, child: &ProcessIdentity) -> io::Result<Snapshot> {
        map(self
            .open()?
            .ownership_record_spawn(fence, child, self.until))
    }
    pub(super) fn accept(&self, fence: &Fence) -> io::Result<Snapshot> {
        map(self.open()?.ownership_record_acceptance(fence, self.until))
    }
}
impl Commands<'_> {
    pub(super) fn capture_begin(
        &self,
        binding: &hf_domain::capture::CaptureBinding,
    ) -> io::Result<hf_domain::capture::CaptureSnapshot> {
        map(self.open()?.capture_begin(binding, self.until))
    }
    pub(super) fn capture_publish(
        &self,
        binding: &hf_domain::capture::CaptureBinding,
        revision: u64,
        next: &hf_domain::capture::CaptureProgress,
    ) -> io::Result<hf_domain::capture::CaptureSnapshot> {
        map(self
            .open()?
            .capture_publish(binding, revision, next, self.until))
    }
    pub(super) fn capture_finish(
        &self,
        binding: &hf_domain::capture::CaptureBinding,
        revision: u64,
        exit: hf_domain::process::ProcessExit,
    ) -> io::Result<hf_domain::capture::CaptureSnapshot> {
        map(self
            .open()?
            .capture_finish(binding, revision, exit, self.until))
    }
}

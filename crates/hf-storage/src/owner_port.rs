//! Narrow retained-owner adapter; transactions remain bounded to each call.
use crate::{Error, Store};
use hf_application::owner::OwnerPort;
use hf_domain::{
    capture::*,
    ownership::*,
    process::{ProcessExit, ProcessIdentity},
};
use std::time::Instant;
impl OwnerPort for Store {
    type Error = Error;
    fn observe(&self, operation: &Id, deadline: Instant) -> Result<Option<Snapshot>, Error> {
        self.ownership_observe(operation, deadline)
    }
    fn claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        deadline: Instant,
    ) -> Result<ClaimResult, Error> {
        self.ownership_claim(operation, generation, revision, owner, deadline)
    }
    fn record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<Snapshot, Error> {
        self.ownership_record_spawn(fence, child, deadline)
    }
    fn record_acceptance(&self, fence: &Fence, deadline: Instant) -> Result<Snapshot, Error> {
        self.ownership_record_acceptance(fence, deadline)
    }
    fn capture_begin(
        &self,
        binding: &CaptureBinding,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Error> {
        Store::capture_begin(self, binding, deadline)
    }
    fn capture_publish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        next: &CaptureProgress,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Error> {
        Store::capture_publish(self, binding, expected_revision, next, deadline)
    }
    fn capture_finish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        exit: ProcessExit,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Error> {
        Store::capture_finish(self, binding, expected_revision, exit, deadline)
    }
}

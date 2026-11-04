use crate::{Error, Store};
use hf_application::ownership::OwnershipPort;
use hf_domain::{ownership::*, process::ProcessIdentity};
use std::time::Instant;
impl OwnershipPort for Store {
    type Error = Error;
    type Deadline = Instant;
    fn prepare(
        &self,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<PrepareResult, Error> {
        self.ownership_prepare(request, deadline)
    }
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
    fn cancel(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        deadline: Instant,
    ) -> Result<Snapshot, Error> {
        self.ownership_cancel(operation, generation, revision, deadline)
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
    fn release(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        fence: Option<&Fence>,
        proof: &ReleaseProof,
        deadline: Instant,
    ) -> Result<Snapshot, Error> {
        self.ownership_release(operation, generation, revision, fence, proof, deadline)
    }
}

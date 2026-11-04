//! Constructor-injected internal fixture service; no runtime or process effects.
use hf_domain::{ownership::*, process::ProcessIdentity};
pub trait OwnershipPort {
    type Error;
    type Deadline: Copy;
    fn prepare(
        &self,
        request: &PreparedRequest,
        deadline: Self::Deadline,
    ) -> Result<PrepareResult, Self::Error>;
    fn observe(
        &self,
        operation: &Id,
        deadline: Self::Deadline,
    ) -> Result<Option<Snapshot>, Self::Error>;
    fn claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        deadline: Self::Deadline,
    ) -> Result<ClaimResult, Self::Error>;
    fn cancel(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        deadline: Self::Deadline,
    ) -> Result<Snapshot, Self::Error>;
    fn record_spawn(
        &self,
        fence: &Fence,
        child: &hf_domain::process::ProcessIdentity,
        deadline: Self::Deadline,
    ) -> Result<Snapshot, Self::Error>;
    fn record_acceptance(
        &self,
        fence: &Fence,
        deadline: Self::Deadline,
    ) -> Result<Snapshot, Self::Error>;
    fn release(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        fence: Option<&Fence>,
        proof: &ReleaseProof,
        deadline: Self::Deadline,
    ) -> Result<Snapshot, Self::Error>;
}
pub struct OwnershipService<P> {
    port: P,
}
impl<P: OwnershipPort> OwnershipService<P> {
    pub fn new(port: P) -> Self {
        Self { port }
    }
    pub fn port(&self) -> &P {
        &self.port
    }
}

impl<P: OwnershipPort> OwnershipService<P> {
    pub fn prepare(
        &self,
        request: &PreparedRequest,
        deadline: P::Deadline,
    ) -> Result<PrepareResult, P::Error> {
        self.port.prepare(request, deadline)
    }
    pub fn observe(
        &self,
        operation: &Id,
        deadline: P::Deadline,
    ) -> Result<Option<Snapshot>, P::Error> {
        self.port.observe(operation, deadline)
    }
    pub fn claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        deadline: P::Deadline,
    ) -> Result<ClaimResult, P::Error> {
        self.port
            .claim(operation, generation, revision, owner, deadline)
    }
    pub fn cancel(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        deadline: P::Deadline,
    ) -> Result<Snapshot, P::Error> {
        self.port.cancel(operation, generation, revision, deadline)
    }
    pub fn record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        deadline: P::Deadline,
    ) -> Result<Snapshot, P::Error> {
        self.port.record_spawn(fence, child, deadline)
    }
    pub fn record_acceptance(
        &self,
        fence: &Fence,
        deadline: P::Deadline,
    ) -> Result<Snapshot, P::Error> {
        self.port.record_acceptance(fence, deadline)
    }
    pub fn release(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        fence: Option<&Fence>,
        proof: &ReleaseProof,
        deadline: P::Deadline,
    ) -> Result<Snapshot, P::Error> {
        self.port
            .release(operation, generation, revision, fence, proof, deadline)
    }
}

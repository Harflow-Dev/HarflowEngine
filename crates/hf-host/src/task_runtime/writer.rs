//! Sole-writer transaction spine. Errors poison permanently; handles remain retained.
use super::{Error, HeldRoot};
use hf_application::{
    task_lifecycle::DurableCaptureAdmission,
    task_runtime::{AcceptanceAdmission, InitialIntentValidator, PreAgentRefusalAdmission},
};
use hf_domain::{
    ownership::{OwnerClaim, Phase, PreparedRequest},
    process::ProcessIdentity,
    task_lifecycle::{LifecycleDelta, LifecycleEvidence},
    task_runtime::AcceptanceReadiness,
};
use hf_storage::{LifecycleEventPublication, Store, TaskCreate, TerminalPublication};
use std::time::Instant;

pub struct Writer {
    // Store/raw must close before the root exclusion; no transport owns this connection.
    store: Store,
    evidence: LifecycleEvidence,
    hold: HeldRoot,
    poisoned: bool,
}
impl Writer {
    /// A new writer validates the immutable prepared bytes and actual direct owner.
    /// This does not claim, accept or spawn anything.
    pub fn open(
        hold: HeldRoot,
        request: &PreparedRequest,
        owner: &OwnerClaim,
        deadline: Instant,
    ) -> Result<Self, Error> {
        if hf_platform::process::current_parent_identity().as_ref() != Some(&owner.process) {
            return Err(Error::Refused);
        }
        let invitation = hf_interfaces::task_runtime::decode_invitation(&request.payload)
            .map_err(|_| Error::Refused)?;
        if &invitation.source != hold.source() {
            return Err(Error::Refused);
        }
        let owner_chain =
            hf_platform::task_runtime::caller::observe_caller(owner.process.pid(), deadline)?;
        if owner_chain.chain.first().map(|link| &link.identity) != Some(&owner.process)
            || owner_chain.chain.get(1).map(|link| &link.identity) != Some(&invitation.dispatcher)
            || owner_chain.chain.get(2).map(|link| &link.identity) != Some(&invitation.caller)
        {
            return Err(Error::Refused);
        }
        let store = hold.open_store(deadline)?;
        super::owner_entry::validate_dispatcher(&store, request, &invitation, deadline)?;
        let intent = store
            .read_launch_intent(&invitation.binding, deadline)?
            .ok_or(Error::Refused)?;
        hf_interfaces::task_runtime::CanonicalLaunchValidator
            .validate(request, &intent.create)
            .map_err(|_| Error::Refused)?;
        let evidence = store
            .read_lifecycle(&invitation.binding, deadline)?
            .ok_or(Error::Refused)?;
        if intent.preparation != *request
            || evidence.ownership.request != *request
            || evidence.ownership.phase != Phase::Prepared
            || evidence.revision != 0
            || evidence.ownership.owner.is_some()
            || evidence.ownership.child.is_some()
            || evidence.cancel.is_some()
        {
            return Err(Error::Refused);
        }
        hold.revalidate()?;
        if hf_platform::task_runtime::caller::observe_caller(owner.process.pid(), deadline)?
            != owner_chain
        {
            return Err(Error::Refused);
        }
        Ok(Self {
            store,
            evidence,
            hold,
            poisoned: false,
        })
    }
    pub fn evidence(&self) -> &LifecycleEvidence {
        &self.evidence
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn poison(&mut self) {
        self.poisoned = true;
    }
    fn begin(&mut self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        // Mark uncertain before the first effect, including root revalidation failure.
        self.poisoned = true;
        self.hold.revalidate()
    }
    fn committed(&mut self, result: hf_storage::Result<LifecycleEvidence>) -> Result<(), Error> {
        let next = result?;
        if next.binding != self.evidence.binding || next.revision != self.evidence.revision + 1 {
            return Err(Error::Poisoned);
        }
        self.hold.revalidate()?;
        self.evidence = next;
        self.poisoned = false;
        Ok(())
    }
    pub fn claim(&mut self, owner: OwnerClaim, deadline: Instant) -> Result<(), Error> {
        self.begin()?;
        if hf_platform::process::current_parent_identity().as_ref() != Some(&owner.process) {
            return Err(Error::Refused);
        }
        let result = self.store.apply_lifecycle(
            &self.evidence.binding,
            None,
            self.evidence.revision,
            &LifecycleDelta::OwnerClaimed(owner),
            deadline,
        );
        self.committed(result)
    }
    pub fn publish_spawn(
        &mut self,
        task: &TaskCreate,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.begin()?;
        let fence = self.evidence.ownership.fence().ok_or(Error::Refused)?;
        let publication = hf_storage::SpawnPublication {
            task,
            child,
            projection: &hf_interfaces::task_runtime::CanonicalLaunchValidator,
        };
        let result = self.store.publish_launch_spawn(
            &self.evidence.binding,
            &fence,
            self.evidence.revision,
            &publication,
            deadline,
        );
        self.committed(result)
    }
    pub fn accept(
        &mut self,
        readiness: &AcceptanceReadiness,
        admission: &dyn AcceptanceAdmission,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.begin()?;
        let fence = self.evidence.ownership.fence().ok_or(Error::Refused)?;
        let result = self.store.record_launch_acceptance(
            &self.evidence.binding,
            &fence,
            self.evidence.revision,
            readiness,
            admission,
            deadline,
        );
        self.committed(result)
    }
    pub fn publish_capture(
        &mut self,
        observation: &hf_domain::task_lifecycle::ExecutionObservation,
        events: &[hf_storage::EventAppend],
        admission: &dyn DurableCaptureAdmission,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.begin()?;
        let fence = self.evidence.ownership.fence().ok_or(Error::Refused)?;
        let publication = LifecycleEventPublication {
            events,
            capture: admission,
        };
        let result = self.store.apply_lifecycle_events(
            &self.evidence.binding,
            Some(&fence),
            self.evidence.revision,
            &LifecycleDelta::ObservationCommitted(observation.clone()),
            &publication,
            deadline,
        );
        self.committed(result)
    }
    pub fn finish(
        &mut self,
        publication: &TerminalPublication<'_>,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.begin()?;
        let fence = self.evidence.ownership.fence().ok_or(Error::Refused)?;
        let result = self.store.publish_task_terminal(
            &self.evidence.binding,
            &fence,
            self.evidence.revision,
            publication,
            deadline,
        );
        self.committed(result)
    }
    pub fn not_started(
        &mut self,
        reason: &str,
        admission: &dyn PreAgentRefusalAdmission,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.begin()?;
        let fence = self.evidence.ownership.fence();
        let result = self.store.record_launch_not_started(
            &self.evidence.binding,
            fence.as_ref(),
            self.evidence.revision,
            reason,
            admission,
            deadline,
        );
        self.committed(result)
    }
}

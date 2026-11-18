//! Fresh single-thread owner entry. No backend effect occurs during bootstrap.
use super::{Error, HeldRoot};
use hf_application::{owner_runtime::DigestPort, task_runtime::InitialIntentValidator};
use hf_domain::{
    ownership::PreparedRequest, process::ProcessIdentity, task_runtime::ProductionInvitation,
};
use hf_platform::task_runtime::{bootstrap::InheritedStdio, channel::Channel};
use std::{fs::File, path::Path, time::Instant};

/// Retain for the complete owner lifetime, including uncertain probe/writer cleanup.
/// The application loop has sole custody of these endpoints and root exclusion.
pub struct OwnerEntry {
    invitation: ProductionInvitation,
    request: PreparedRequest,
    identity: ProcessIdentity,
    hold: HeldRoot,
    channel: Channel,
    _error: File,
}
impl OwnerEntry {
    /// Must be called directly from a fresh private-mode entry before any other FDs
    /// or threads. The root path is only a locator: physical source identity must match.
    pub fn enter(root: &Path, deadline: Instant) -> Result<Self, Error> {
        let stdio = InheritedStdio::acquire(deadline)?;
        let frame = stdio.read_frame(deadline, 16384)?;
        let payload = frame.get(4..).ok_or(Error::Refused)?;
        let invitation =
            hf_interfaces::task_runtime::decode_invitation(payload).map_err(|_| Error::Refused)?;
        stdio.qualify(
            &invitation.dispatcher,
            &invitation.ctl,
            &hf_storage::Sha256Digest,
            deadline,
        )?;
        let identity = stdio.observation().process.clone();
        let hold = HeldRoot::open(root, &invitation, deadline)?;
        stdio.revalidate(deadline)?;
        let request = admit_invitation(payload, &invitation, &hold, deadline)?;
        let cwd = std::fs::canonicalize(&invitation.cwd)?;
        if cwd != Path::new(&invitation.cwd) || !cwd.is_dir() {
            return Err(Error::Refused);
        }
        std::env::set_current_dir(cwd)?;
        let session = hf_platform::task_runtime::enter_owner_session()?;
        if session != identity.pid() {
            return Err(Error::Refused);
        }
        stdio.revalidate(deadline)?;
        let (channel, error) = stdio.into_channel(24580)?;
        Ok(Self {
            invitation,
            request,
            identity,
            hold,
            channel,
            _error: error,
        })
    }
    pub fn invitation(&self) -> &ProductionInvitation {
        &self.invitation
    }
    pub fn request(&self) -> &PreparedRequest {
        &self.request
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    pub fn hold(&self) -> &HeldRoot {
        &self.hold
    }
    pub fn channel(&mut self) -> &mut Channel {
        &mut self.channel
    }
}
/// Builds the prepared request for the invitation payload and validates it against the
/// stored intent and lifecycle evidence under the held root.
pub(super) fn admit_invitation(
    payload: &[u8],
    invitation: &ProductionInvitation,
    hold: &HeldRoot,
    deadline: Instant,
) -> Result<PreparedRequest, Error> {
    let request = PreparedRequest {
        operation: invitation.binding.operation.clone(),
        session: invitation.ownership_session.clone(),
        reservation: invitation.reservation.clone(),
        attempt: invitation.binding.attempt.clone(),
        generation: invitation.binding.generation.clone(),
        digest: hf_storage::Sha256Digest.digest_parts(&[payload]),
        payload: payload.to_vec(),
    };
    validate_prepared(hold, &request, invitation, deadline)?;
    Ok(request)
}
fn validate_prepared(
    hold: &HeldRoot,
    request: &PreparedRequest,
    invitation: &ProductionInvitation,
    deadline: Instant,
) -> Result<(), Error> {
    let store = hold.open_validation_store(deadline)?;
    let intent = store
        .read_launch_intent(&invitation.binding, deadline)?
        .ok_or(Error::Refused)?;
    let evidence = store
        .read_lifecycle(&invitation.binding, deadline)?
        .ok_or(Error::Refused)?;
    if intent.preparation != *request
        || evidence.ownership.request != *request
        || evidence.ownership.phase != hf_domain::ownership::Phase::Prepared
        || evidence.revision != 0
        || evidence.cancel.is_some()
        || evidence.ownership.owner.is_some()
        || evidence.ownership.child.is_some()
    {
        return Err(Error::Refused);
    }
    hf_interfaces::task_runtime::CanonicalLaunchValidator
        .validate(request, &intent.create)
        .map_err(|_| Error::Refused)?;
    validate_dispatcher(&store, request, invitation, deadline)?;
    hold.revalidate()
}
pub(super) fn validate_dispatcher(
    store: &hf_storage::Store,
    request: &PreparedRequest,
    invitation: &ProductionInvitation,
    deadline: Instant,
) -> Result<(), Error> {
    use hf_platform::task_runtime::caller::observe_caller;
    let before = observe_caller(invitation.dispatcher.pid(), deadline)?;
    if before.chain.first().map(|link| &link.identity) != Some(&invitation.dispatcher)
        || before.chain.get(1).map(|link| &link.identity) != Some(&invitation.caller)
    {
        return Err(Error::Refused);
    }
    let denied = store.runtime_mutation_candidates_except(
        &hf_interfaces::task_runtime::CanonicalLaunchValidator,
        64,
        request,
        deadline,
    )?;
    if before
        .chain
        .iter()
        .any(|link| denied.contains(&link.identity))
    {
        return Err(Error::Refused);
    }
    if observe_caller(invitation.dispatcher.pid(), deadline)? != before {
        return Err(Error::Refused);
    }
    Ok(())
}

//! Pure validation for the private fixture handoff boundary.
use hf_domain::{
    fixture::{Invitation, Prepared},
    ownership::{Phase, Snapshot},
    process::ProcessIdentity,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Binding,
    StoredRequest,
    State,
}
pub fn validate_prepared(
    invitation: &Invitation,
    prepared: &Prepared,
    actual: &ProcessIdentity,
    stored: &Snapshot,
) -> Result<(), Refusal> {
    if invitation != &prepared.invitation
        || &invitation.process != actual
        || invitation.operation != prepared.request.operation
        || invitation.generation != prepared.request.generation
        || invitation.digest != prepared.request.digest
    {
        return Err(Refusal::Binding);
    }
    if stored.request != prepared.request || stored.revision != invitation.revision {
        return Err(Refusal::StoredRequest);
    }
    if stored.phase != Phase::Prepared
        || stored.cancelled
        || stored.owner.is_some()
        || stored.child.is_some()
    {
        return Err(Refusal::State);
    }
    Ok(())
}
pub fn validate_accepted(
    invitation: &Invitation,
    fence: &hf_domain::ownership::Fence,
    child: &ProcessIdentity,
    actual_owner: &ProcessIdentity,
    stored: &Snapshot,
) -> Result<(), Refusal> {
    if &invitation.process != actual_owner
        || stored.fence().as_ref() != Some(fence)
        || stored.child.as_ref() != Some(child)
        || fence.owner.process != *actual_owner
        || fence.owner.owner != invitation.owner
        || fence.owner.nonce != invitation.nonce
        || fence.operation != invitation.operation
        || fence.generation != invitation.generation
        || stored.request.digest != invitation.digest
    {
        return Err(Refusal::Binding);
    }
    if stored.phase != Phase::AcceptanceRecorded {
        return Err(Refusal::State);
    }
    Ok(())
}

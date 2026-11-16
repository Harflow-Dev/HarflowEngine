//! Binding checks under an original, still-held maintenance session.
//! Passing these checks grants no eligibility, inventory or owner-evidence authority.
use hf_domain::ctl::recovery_proposal::{FileIdentity, RecoveryProposal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    Stale,
    ActiveOwner,
    Unavailable,
}

/// Implementations retain the original exclusive guards throughout every call.
pub trait HeldSessionPort {
    fn verify_held(&self) -> Result<(), BindingError>;
    fn state_root(&self) -> &str;
    fn held_root_identity(&self) -> FileIdentity;
    fn held_lock_identity(&self) -> FileIdentity;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingMode {
    Fresh,
    Resume { lock_identity: FileIdentity },
}

/// Exact selected request and still-held root binding; fresh null locks normalize only
/// the proposal comparison. Resume always names the actual journaled lock.
pub fn verify_binding(
    session: &dyn HeldSessionPort,
    proposal: &RecoveryProposal,
    selected_root: &str,
    engine_version: &str,
    operation_id: &str,
    mode: BindingMode,
) -> Result<(), BindingError> {
    session.verify_held()?;
    let expected_lock = match mode {
        BindingMode::Fresh => proposal.lock_identity,
        BindingMode::Resume { lock_identity } => Some(lock_identity),
    };
    if proposal.state_root != selected_root
        || proposal.engine_version != engine_version
        || proposal.operation_id != operation_id
        || session.state_root() != selected_root
        || session.held_root_identity() != proposal.root_identity
        || proposal
            .lock_identity
            .is_some_and(|lock| lock != session.held_lock_identity())
        || expected_lock.is_some_and(|lock| lock != session.held_lock_identity())
    {
        return Err(BindingError::Stale);
    }
    Ok(())
}

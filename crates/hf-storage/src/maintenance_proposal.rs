//! Acquisition bound to a decoded confirmation proposal; no caller identity is invented.
use crate::{MaintenanceSession, SessionRequest};
use hf_application::{
    ctl::state_recover::{RecoveryFsPort, flow::RecoveryError},
    lease::LeasePort,
};
use hf_domain::ctl::recovery_proposal::RecoveryProposal;
use std::sync::Arc;
impl MaintenanceSession {
    pub fn acquire_proposal(
        proposal: &RecoveryProposal,
        fs: Arc<dyn RecoveryFsPort>,
        leases: Arc<dyn LeasePort>,
    ) -> std::result::Result<Self, RecoveryError> {
        let identity = |id: hf_domain::ctl::recovery_proposal::FileIdentity| {
            hf_domain::capture::FileIdentity {
                device: id.dev as u64,
                inode: id.ino,
            }
        };
        Self::acquire(
            &proposal.state_root,
            &SessionRequest {
                root_identity: identity(proposal.root_identity),
                lock_identity: proposal.lock_identity.map(identity),
            },
            fs,
            leases,
        )
        .map_err(|error| match error {
            crate::Error::Busy => RecoveryError::ActiveOwner,
            crate::Error::Replaced | crate::Error::InvalidRoot => RecoveryError::StaleConfirmation,
            _ => RecoveryError::InfrastructureUnavailable,
        })
    }
}

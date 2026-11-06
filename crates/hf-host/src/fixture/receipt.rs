//! One-use original private handoff. No file or row can manufacture this port.
use super::*;
use hf_application::{prepared_launch::ReceiptDeliveryPort, recovery::RecoveryError};
use hf_domain::prepared_launch::CaptureAccepted;
use std::cell::Cell;
pub(super) struct Delivery<'a> {
    child: &'a RetainedOwnedChild,
    consumed: Cell<bool>,
}
impl<'a> Delivery<'a> {
    pub(super) fn new(child: &'a RetainedOwnedChild) -> Self {
        Self {
            child,
            consumed: Cell::new(false),
        }
    }
}
impl ReceiptDeliveryPort for Delivery<'_> {
    fn receive(
        &self,
        invitation: &Invitation,
        deadline: Instant,
    ) -> Result<Option<CaptureAccepted>, RecoveryError> {
        if self.consumed.replace(true) {
            return Ok(None);
        }
        if self.child.registered_identity() != Some(&invitation.process) {
            return Err(RecoveryError::Refused);
        }
        let bytes = self
            .child
            .read_fixture_frame(deadline)
            .map_err(|_| RecoveryError::Unavailable)?;
        let frame = decode(&bytes).map_err(|_| RecoveryError::Refused)?;
        self.child
            .fixture_eof(deadline)
            .map_err(|_| RecoveryError::Unavailable)?;
        let Frame::CaptureAccepted(receipt) = frame else {
            return Err(RecoveryError::Refused);
        };
        if &receipt.invitation != invitation
            || receipt.ownership.owner.as_ref().map(|o| &o.process) != Some(&invitation.process)
        {
            return Err(RecoveryError::Refused);
        }
        Ok(Some(*receipt))
    }
}

//! Read-only point-in-time evidence; never process control or recovery authority.
mod identity;
mod raw;
pub use identity::MacRecoveryIdentity;
pub use raw::RawCaptureObserver;

use hf_application::recovery::RecoveryError;
use std::time::Instant;

fn timely(deadline: Instant) -> Result<(), RecoveryError> {
    if Instant::now() >= deadline {
        Err(RecoveryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

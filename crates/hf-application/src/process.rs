//! Injected observation only; concrete process effects belong to platform.
use hf_domain::process::{IdentityObservation, ProcessIdentity};
pub trait IdentityPort {
    fn identity(&self, pid: u32) -> Option<ProcessIdentity>;
    fn observe(&self, expected: &ProcessIdentity) -> IdentityObservation {
        match self.identity(expected.pid()) {
            Some(actual) if actual == *expected => IdentityObservation::Matching,
            Some(_) => IdentityObservation::Mismatch,
            None => IdentityObservation::Unknown,
        }
    }
}

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Ports and bootstrap use cases. Concrete adapters are composed by the host.
use hf_domain::Readiness;
pub trait ReadinessPort {
    fn readiness(&self) -> Readiness;
}
pub fn read_readiness(port: &impl ReadinessPort) -> Readiness {
    port.readiness()
}

pub mod task_read;

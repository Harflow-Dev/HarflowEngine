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

pub mod event_read;

pub mod process;

pub mod ownership;

pub mod fixture;

#[cfg(test)]
#[path = "fixture_tests.rs"]
mod fixture_tests;

pub mod task_watch;

pub mod recovery;

pub mod lease;

pub mod owner;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod owner_tests;

pub mod owner_runtime;

pub mod owner_policy;

pub mod backend_runtime;

pub mod prepared_launch;

pub mod conversation;

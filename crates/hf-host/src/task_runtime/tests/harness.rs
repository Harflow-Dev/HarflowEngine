//! Shared helpers of the task runtime cases.
use hf_domain::ownership::Id;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) use super::{cleanup::Cleanup, owner_case::*};
/// The unwind payload that leaves a retention loop.
pub(in crate::task_runtime) struct Retained;

pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
pub(super) fn leases() -> Arc<dyn hf_application::lease::LeasePort> {
    hf_platform::lease::shared()
}
pub(super) fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}
/// Dispatcher-level cases read the process environment: it must not carry task variables.
pub(super) fn require_clean_environment() {
    for key in [
        "HARFLOW_TASK_ID",
        "HARFLOW_ROOT_TASK_ID",
        "HARFLOW_DEPTH",
        "PB_TASK_ID",
        "PB_ROOT_TASK_ID",
        "PB_DEPTH",
    ] {
        assert!(
            std::env::var_os(key).is_none(),
            "precondition: {key} must be unset; run under env -u"
        );
    }
}

pub(super) use super::probe::{Probe, Seed, open_reader};
pub(super) use super::{ctx::Ctx, held::*, writer_config::*};

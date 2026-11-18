//! The single entry for caller observations of the task runtime.
use hf_platform::task_runtime::caller::CallerObservation;
use std::{io, time::Instant};

/// Observes the process `pid` and its ancestry.
pub(super) fn observe(pid: u32, deadline: Instant) -> io::Result<CallerObservation> {
    #[cfg_attr(test, allow(unused_variables))]
    let observe = hf_platform::task_runtime::caller::observe_caller;
    #[cfg(test)]
    let observe = super::seams::observe_caller;
    observe(pid, deadline)
}
/// Observes the current process and its ancestry.
pub(super) fn current(deadline: Instant) -> io::Result<CallerObservation> {
    observe(std::process::id(), deadline)
}

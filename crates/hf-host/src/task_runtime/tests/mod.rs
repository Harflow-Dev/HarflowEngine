#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Crate-internal lifecycle qualification of injected cases (E2-C S2).
mod ack_loss;
mod agents;
mod burst_cases;
mod caller_policy;
mod caller_source;
mod cleanup;
mod ctx;
mod driver;
mod flush;
mod harness;
mod held;
mod observer;
mod owner_case;
mod owner_cases;
mod owner_process_cases;
mod probe;
mod retained_cause_cases;
mod root_checks;
mod stall_policy;
pub(super) mod thread_writer;
mod writer_cases;
mod writer_config;

pub(in crate::task_runtime) use crate::test_lock::serial_case;

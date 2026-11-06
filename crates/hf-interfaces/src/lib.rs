#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Diagnostic formatting only; no CLI or MCP production protocol.
pub mod event_consumer;
pub mod task_consumer;

#[cfg(test)]
#[path = "fixture_tests.rs"]
mod fixture_tests;
use hf_domain::Readiness;
pub fn describe(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Unsupported => "experimental scaffold; engine operations unsupported",
    }
}

pub mod fixture;

pub mod watch_codec;
pub mod watch_consumer;

pub mod owner_storage_codec;

pub mod owner_profile;

pub mod owner_preparation;

pub mod backend_measurement;

pub mod claude_stream;

pub mod prepared_launch;

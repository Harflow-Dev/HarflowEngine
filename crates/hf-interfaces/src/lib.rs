#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Diagnostic formatting only; no CLI or MCP production protocol.
pub mod task_consumer;
use hf_domain::Readiness;
pub fn describe(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Unsupported => "experimental scaffold; engine operations unsupported",
    }
}

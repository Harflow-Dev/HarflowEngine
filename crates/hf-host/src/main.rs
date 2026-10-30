#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Composition root for the explicitly unsupported scaffold.
use hf_application::{ReadinessPort, read_readiness};
use hf_domain::Readiness;
struct UnsupportedRuntime;
impl ReadinessPort for UnsupportedRuntime {
    fn readiness(&self) -> Readiness {
        Readiness::Unsupported
    }
}
fn main() {
    println!(
        "harflow-engine-scaffold: {}",
        hf_interfaces::describe(read_readiness(&UnsupportedRuntime))
    );
}

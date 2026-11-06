#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Composition root for the explicitly unsupported scaffold.
use hf_application::{ReadinessPort, read_readiness};
use hf_domain::Readiness;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod fixture;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod owner_runtime;
struct UnsupportedRuntime;
impl ReadinessPort for UnsupportedRuntime {
    fn readiness(&self) -> Readiness {
        Readiness::Unsupported
    }
}
fn main() {
    let mut arguments = std::env::args_os();
    let _ = arguments.next();
    if let Some(mode) = arguments.next() {
        if arguments.next().is_some() {
            private_refusal();
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(mode) = mode.to_str()
            && matches!(
                mode,
                "--fixture-fixed-protocol-helper"
                    | hf_domain::claude_qualification::PARENT_MODE
                    | hf_domain::claude_qualification::BRIDGE_MODE
                    | "--fixture-exec-bridge-witness-v1"
                    | "--fixture-exec-image-witness-v1"
                    | "--internal-parent-fixture-v1"
                    | "--internal-owner-fixture-v1"
                    | "--internal-effect-fixture-v1"
                    | "--internal-effect-saturation-fixture-v1"
            )
        {
            if fixture::run(mode).is_err() {
                private_refusal();
            }
            return;
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(mode) = mode.to_str()
            && matches!(
                mode,
                "--internal-storage-writer-fixture-v1"
                    | "--internal-isolated-owner-fixture-v1"
                    | hf_domain::claude_qualification::OWNER_MODE
                    | hf_domain::claude_qualification::WRITER_MODE
            )
        {
            if owner_runtime::run(mode).is_err() {
                private_refusal();
            }
            return;
        }
        let _ = mode;
        private_refusal();
    }
    println!(
        "harflow-engine-scaffold: {}",
        hf_interfaces::describe(read_readiness(&UnsupportedRuntime))
    );
}
fn private_refusal() -> ! {
    // Entry may have failed because stdio is not private. Touch no unqualified
    // endpoint and do not risk a panic or blocked diagnostic after parent loss.
    // Exit 2 is a private protocol refusal, never proof of non-execution.
    std::process::exit(2)
}

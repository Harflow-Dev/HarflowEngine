#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `harflow-ctl`: the public read-only skeleton. It composes the state observer with
//! the ctl transport mapping and writes nothing to the state root.
mod services;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod store_reads;
use std::{io::Write, process::ExitCode};
fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let outcome = hf_interfaces::ctl::dispatch::dispatch(&argv, &services::HostServices);
    // Output failures such as a closed pipe must not panic or change the exit code.
    let _ = std::io::stdout().write_all(outcome.stdout.as_bytes());
    let _ = std::io::stderr().write_all(outcome.stderr.as_bytes());
    let _ = std::io::stdout().flush();
    ExitCode::from(u8::try_from(outcome.code).unwrap_or(1))
}

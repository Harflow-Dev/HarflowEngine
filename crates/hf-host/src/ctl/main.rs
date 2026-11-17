#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `harflow-ctl`: public observation, reads and explicitly confirmed recovery.
mod services;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod state_recover;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod store_history;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod store_pending;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod store_reads;
use std::{io::Write, process::ExitCode};
fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if let Some(code) = private_entry(&argv) {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let outcome = hf_interfaces::ctl::dispatch::dispatch(&argv, &services::HostServices::new());
    // Output failures such as a closed pipe must not panic or change the exit code.
    let _ = std::io::stdout().write_all(outcome.stdout.as_bytes());
    let _ = std::io::stderr().write_all(outcome.stderr.as_bytes());
    let _ = std::io::stdout().flush();
    ExitCode::from(u8::try_from(outcome.code).unwrap_or(1))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn private_entry(argv: &[String]) -> Option<i32> {
    match argv.first().map(String::as_str) {
        Some("__task-owner") if argv.len() == 2 && std::path::Path::new(&argv[1]).is_absolute() => {
            Some(hf_host::task_runtime::run_owner(argv[1].clone().into()))
        }
        Some("__task-writer") if argv.len() == 1 => Some(hf_host::task_runtime::run_writer()),
        Some("__task-owner" | "__task-writer") => Some(2),
        _ => None,
    }
}

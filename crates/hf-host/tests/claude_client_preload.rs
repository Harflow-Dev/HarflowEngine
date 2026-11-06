#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#[path = "claude_two_tunnel/run.rs"]
mod run;
#[test]
#[ignore = "explicitly authorized inert version-only preload support probe; never provider access"]
fn installed_claude_client_preload_support() {
    run::run_client_preload_probe();
}

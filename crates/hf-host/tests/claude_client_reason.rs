#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "claude_two_tunnel/run.rs"]
mod run;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "explicit pinned fake-only client reason diagnostic"]
fn fake_only_client_reason() {
    run::run_client_reason();
}

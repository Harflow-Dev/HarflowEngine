#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#[path = "claude_two_tunnel/run.rs"]
mod run;
#[test]
#[ignore = "explicitly authorized fake-only TLS milestones; never real credentials"]
fn installed_claude_handshake_eof_diagnostic() {
    run::run_handshake_diagnostic();
}

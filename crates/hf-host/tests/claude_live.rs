#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "claude_live/phases.rs"]
mod phases;

#[test]
#[ignore = "authorized billable primary-only diagnostic; requires pinned Claude 2.1.318 and the current account credential"]
fn installed_claude_live_authentication_egress() {
    phases::run();
}

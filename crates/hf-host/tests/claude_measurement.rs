#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Private unpaid Claude fixture behind an ignored installed-vendor measurement test.
#[path = "claude_measurement/support.rs"]
mod claude_measurement;

#[test]
#[ignore = "requires explicit Claude 2.1.318 selection; runs same-profile preflight before the local fixture CLI"]
fn installed_claude_fixture_after_no_fork_read_and_network_preflight() {
    claude_measurement::vendor_measurement_preflight();
}

#[test]
#[ignore = "diagnostic only; explicit vendor selection and primary OS readiness required; stops after version query"]
fn installed_claude_exact_os_banner_read_version_only() {
    claude_measurement::vendor_version_only_diagnostic();
}

#[test]
#[ignore = "diagnostic only; explicit vendor selection and primary OS readiness required; stops after version query"]
fn installed_claude_pagesize_read_version_only() {
    claude_measurement::vendor_pagesize_version_only_diagnostic();
}

#[test]
#[ignore = "diagnostic only; explicit vendor selection and primary OS readiness required; stops after version query"]
fn installed_claude_pagesize_compat_read_version_only() {
    claude_measurement::vendor_pagesize_compat_version_only_diagnostic();
}

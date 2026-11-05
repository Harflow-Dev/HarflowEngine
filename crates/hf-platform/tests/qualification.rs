#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic)]

//! Actual OS measurement probes are deliberately opt-in. They use only fresh
//! roots created by FixtureRoot and settle each safe-spawn child before removal.
mod qualification_support;

#[test]
fn qualification_platform_requires_the_measured_platform() {
    hf_platform::qualification::require_support().expect("macOS arm64 measurement host");
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn actual_path_boundary_and_alias_counterexample() {
    qualification_support::path_boundary_and_alias_counterexample();
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn actual_ancestor_rename_counterexample() {
    qualification_support::ancestor_rename_counterexample();
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn vendor_no_fork_profile_denies_both_fork_apis() {
    qualification_support::vendor_no_fork_profile_denies_both_fork_apis();
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn controlled_direct_descendant_route_is_measured() {
    qualification_support::controlled_route("descendant");
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn controlled_shell_route_is_measured() {
    qualification_support::controlled_route("shell");
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn controlled_alternate_exec_route_is_measured() {
    qualification_support::controlled_route("alternate-exec");
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn controlled_nested_sandbox_route_is_measured() {
    qualification_support::controlled_route("nested-sandbox");
}

#[test]
fn topology_guard_refuses_existing_aliases_and_special_files() {
    qualification_support::topology_guard_refuses_aliases();
}

#[test]
fn topology_guard_refuses_replaced_role_directories() {
    qualification_support::topology_guard_refuses_replaced_roles();
}

#[test]
#[ignore = "requires the explicitly authorized unsandboxed OS qualification run"]
fn safe_spawn_closes_unlisted_descriptors() {
    qualification_support::safe_spawn_closes_unlisted_descriptors();
}

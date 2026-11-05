#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Internal bounded owned-child primitives. No public runtime capability is implied.
pub mod process;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod fixture;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod recovery;

pub mod lease;

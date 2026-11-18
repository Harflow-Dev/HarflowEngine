#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! The workspace's single reviewed `unsafe` boundary.
//!
//! Every other crate keeps `unsafe_code = "forbid"`. This crate denies it instead and
//! carries exactly one `#[expect(unsafe_code)]`, on the `sysctl` call in `read_into`.
//! The crate is empty on targets other than macOS.

#[cfg(target_os = "macos")]
mod read_into;
#[cfg(target_os = "macos")]
pub use read_into::read_into;

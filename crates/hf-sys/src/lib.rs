#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! The workspace's single reviewed exception to the forbidden-code lint: one sysctl call
//! in `read_into`. Every other crate forbids it; this crate denies it and expects it on
//! that one statement. The crate is empty on targets other than macOS.

#[cfg(target_os = "macos")]
mod read_into;
#[cfg(target_os = "macos")]
pub use read_into::read_into;

//! Isolated ST04 diagnostics. No value in this module authorizes execution/replay.
//! Root ownership is retained on failure and drop; cleanup is always explicit.
pub const HELPER_SOURCE: &str = include_str!("st04_fixture/helper.c");
pub const BUILD_ARGUMENTS: &[&str] = &["-std=c11", "-Wall", "-Wextra", "-Werror", "-O0"];
pub const CAPTURE_LIMIT: usize = 4096;
pub const PROFILE_REVISION: &str = "st04-fixed-absence-v1";
mod measurement;
mod oracles;
mod profile;
pub use measurement::{Diagnostic, HelperLaunch, HelperMeasurement};
pub use profile::{ProbeControl, absence_profile};

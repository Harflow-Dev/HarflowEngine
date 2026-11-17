//! Production composition seams. Public command admission remains separate.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod task_runtime;

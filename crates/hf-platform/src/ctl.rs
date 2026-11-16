//! Filesystem and environment adapters for the public ctl surface.
#[cfg(target_os = "macos")]
mod artifact_authority;
#[cfg(target_os = "macos")]
pub mod artifact_target;
pub mod inventory;
#[cfg(target_os = "macos")]
mod kernel_spelling;
pub mod operation_id;
#[cfg(target_os = "macos")]
pub mod recovery_fs;
pub mod state_observer;
pub mod state_root;

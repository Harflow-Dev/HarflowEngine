//! A caller must retain an unqualified child and arrange explicit qualified cleanup.
//! Dropping closes pipes only; it never signals. No external SIGCHLD reaper may wait
//! on these children. Dedicated initial groups do not prove descendant containment.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod capture;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod identity;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod owned;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod table;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use capture::{Capture, StreamCapture};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use identity::{MacIdentity, current_parent_identity, observed_executable_path};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use owned::*;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use table::PsProcessView;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformSupport {
    MacOsPrimitives,
    Unsupported,
}
pub fn support() -> PlatformSupport {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        PlatformSupport::MacOsPrimitives
    } else {
        PlatformSupport::Unsupported
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod spec;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use spec::SpawnSpec;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod spawn;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod reader;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod reader_loop;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use reader::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod storage_io;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use storage_io::{PrivateStorageIo, StorageStderr};

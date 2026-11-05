//! Diagnostic OS measurements only. These values grant no production admission.
//! Roots are deliberately retained on drop: unknown actors must never trigger deletion.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod fixture;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod profile;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use fixture::{FixtureRoot, PhysicalIdentity};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use profile::{ProfilePolicy, quote_sbpl};

/// This adapter has no weaker fallback on unsupported systems.
pub fn require_support() -> std::io::Result<()> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "OS qualification requires macOS arm64 and measured sandbox-exec; production strict execution remains unsupported",
        ))
    }
}

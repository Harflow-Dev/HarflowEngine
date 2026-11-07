//! Real process identity of a pid, available where the platform adapter is.
use crate::fail::{Fail, Result};
use hf_domain::process::ProcessIdentity;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn read(pid: u32) -> Result<ProcessIdentity> {
    use hf_application::process::IdentityPort;
    hf_platform::process::MacIdentity
        .identity(pid)
        .ok_or_else(|| Fail::new("the process identity of the pid cannot be read"))
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn read(_pid: u32) -> Result<ProcessIdentity> {
    Err(Fail::new(
        "process identity is available on macOS arm64 only",
    ))
}
/// A start identity for the same pid that differs from `real`.
pub fn different(real: &ProcessIdentity) -> Result<ProcessIdentity> {
    let micros = u64::from(real.start_seconds() == EARLIER && real.start_microseconds() == 0);
    ProcessIdentity::new(real.pid(), EARLIER, micros, real.boot_uuid().to_owned())
        .map_err(|_| Fail::new("cannot build a differing process identity"))
}
const EARLIER: u64 = 978_307_200;

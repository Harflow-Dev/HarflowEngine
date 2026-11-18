//! Kernel process-record reader for every ancestry link (owner decision 2026-11-18, option A).
//!
//! libproc refuses root-owned non-init processes for an unprivileged caller, so each record
//! is read from `kern.proc.pid` through hf-sys and decoded with the SDK layout. The layout is
//! selected only on a host whose architecture, `kern.osrelease` and `kern.osversion` are
//! listed in `QUALIFIED`; qualification runs on every read and any other host refuses
//! without reading. There is no libproc fallback. Chain identities are compared elsewhere
//! with libproc-derived identities, so the kernel record must agree with libproc on pid,
//! parent, effective uid and start time wherever libproc can read the process; tests sample
//! that invariant. The kernel call cannot be cancelled: the caller's post-read deadline check
//! is a cooperative bound, as for the earlier reads.
use super::{CallerProcessRecord, CallerRecordLayout, decode_caller_record, unknown};
use nix::libc::{CTL_KERN, KERN_PROC, KERN_PROC_PID};
use std::io;
use sysctl::{Ctl, Sysctl};

/// Reviewed `(kern.osrelease, kern.osversion)` pairs on aarch64. Adding one needs its own
/// layout qualification run and review.
const QUALIFIED: [(&str, &str); 1] = [("25.6.0", "25G83")];
const RECORD_LEN: usize = 648;
const BUFFER_LEN: usize = 1024;

pub(super) trait KernelPlatform {
    fn os_strings(&mut self) -> io::Result<(String, String)>;
    fn read(&mut self, pid: u32, buf: &mut [u8]) -> io::Result<usize>;
}

struct SystemKernel;
impl KernelPlatform for SystemKernel {
    fn os_strings(&mut self) -> io::Result<(String, String)> {
        let string = |name: &str| {
            Ctl::new(name)
                .and_then(|ctl| ctl.value_string())
                .map_err(|_| unknown())
        };
        Ok((string("kern.osrelease")?, string("kern.osversion")?))
    }
    fn read(&mut self, pid: u32, buf: &mut [u8]) -> io::Result<usize> {
        let native = i32::try_from(pid).map_err(|_| unknown())?;
        hf_sys::read_into(&[CTL_KERN, KERN_PROC, KERN_PROC_PID, native], buf)
    }
}

fn qualify(arch: &str, release: &str, version: &str) -> Option<CallerRecordLayout> {
    let listed = QUALIFIED.iter().any(|&(r, v)| r == release && v == version);
    (arch == "aarch64" && listed).then_some(CallerRecordLayout::MacOsArm64Sdk26_5)
}

pub(super) fn read_record(pid: u32) -> io::Result<CallerProcessRecord> {
    read_record_with(&mut SystemKernel, pid)
}

pub(super) fn read_record_with<P: KernelPlatform + ?Sized>(
    platform: &mut P,
    pid: u32,
) -> io::Result<CallerProcessRecord> {
    if pid == 0 || i32::try_from(pid).is_err() {
        return Err(unknown());
    }
    let (release, version) = platform.os_strings().map_err(|_| unknown())?;
    let layout = qualify(std::env::consts::ARCH, &release, &version).ok_or_else(unknown)?;
    let mut buf = [0u8; BUFFER_LEN];
    let length = platform.read(pid, &mut buf).map_err(|_| unknown())?;
    if length != RECORD_LEN {
        return Err(unknown());
    }
    decode_caller_record(layout, pid, &buf[..RECORD_LEN])
}

#[cfg(test)]
mod os_tests;
#[cfg(test)]
mod tests;

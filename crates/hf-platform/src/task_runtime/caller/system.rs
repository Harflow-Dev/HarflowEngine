//! Production evidence source: qualified `kern.proc.pid` records for every link (owner
//! decision 2026-11-18, option A), sysctl boot observations and actual `getuid`.
use super::{CallerEvidenceSource, CallerProcessRecord, kernel_reader, unknown};
use std::{io, time::Instant};
use sysctl::{Ctl, Sysctl};

pub(super) struct SystemSource;
impl CallerEvidenceSource for SystemSource {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn boot_uuid(&mut self) -> io::Result<String> {
        Ctl::new("kern.bootsessionuuid")
            .and_then(|ctl| ctl.value_string())
            .map_err(|_| unknown())
    }
    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord> {
        kernel_reader::read_record(pid)
    }
    fn real_uid(&mut self) -> u32 {
        nix::unistd::getuid().as_raw()
    }
}

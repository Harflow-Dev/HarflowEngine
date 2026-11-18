//! Production evidence source: libproc `BSDInfo` for every non-init process, the qualified
//! `kern.proc.pid` init reader for PID 1, sysctl boot observations and actual `getuid`.
use super::{CallerEvidenceSource, CallerProcessRecord, init_reader, unknown};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use std::{io, time::Instant};
use sysctl::{Ctl, Sysctl};

pub(super) struct LibprocSource;
impl CallerEvidenceSource for LibprocSource {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn boot_uuid(&mut self) -> io::Result<String> {
        Ctl::new("kern.bootsessionuuid")
            .and_then(|ctl| ctl.value_string())
            .map_err(|_| unknown())
    }
    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord> {
        if pid == 1 {
            return init_reader::read_init();
        }
        let native = i32::try_from(pid).map_err(|_| unknown())?;
        let info = pidinfo::<BSDInfo>(native, 0).map_err(|_| unknown())?;
        Ok(CallerProcessRecord {
            pid: info.pbi_pid,
            parent: info.pbi_ppid,
            uid: info.pbi_uid,
            status: info.pbi_status,
            start_seconds: info.pbi_start_tvsec,
            start_microseconds: info.pbi_start_tvusec,
        })
    }
    fn real_uid(&mut self) -> u32 {
        nix::unistd::getuid().as_raw()
    }
}

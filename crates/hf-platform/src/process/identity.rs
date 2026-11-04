use hf_application::process::IdentityPort;
use hf_domain::process::ProcessIdentity;
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use sysctl::{Ctl, Sysctl};
#[derive(Clone, Copy, Debug, Default)]
pub struct MacIdentity;
fn boot_uuid() -> Option<String> {
    Ctl::new("kern.bootsessionuuid").ok()?.value_string().ok()
}
impl IdentityPort for MacIdentity {
    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        let native_pid = i32::try_from(pid).ok().filter(|pid| *pid > 1)?;
        let before = boot_uuid()?;
        let info = pidinfo::<BSDInfo>(native_pid, 0).ok()?;
        let after = boot_uuid()?;
        if before != after || info.pbi_pid != pid {
            return None;
        }
        ProcessIdentity::new(pid, info.pbi_start_tvsec, info.pbi_start_tvusec, before).ok()
    }
}

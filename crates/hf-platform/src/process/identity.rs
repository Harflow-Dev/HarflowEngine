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

/// Observes the direct parent with start/boot evidence, refusing reparenting races.
/// A bare parent PID is never returned as authentication evidence.
pub fn current_parent_identity() -> Option<ProcessIdentity> {
    let before = nix::unistd::getppid().as_raw();
    let pid = u32::try_from(before).ok().filter(|pid| *pid > 1)?;
    let identity = MacIdentity.identity(pid)?;
    if nix::unistd::getppid().as_raw() != before {
        return None;
    }
    let confirmed = MacIdentity.identity(pid)?;
    (confirmed == identity && nix::unistd::getppid().as_raw() == before).then_some(identity)
}

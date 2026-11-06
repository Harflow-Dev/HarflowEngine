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

/// Kernel executable-path observation bound to matching process identity bookends.
/// This is neither a mapped-image digest nor ongoing identity authority.
pub fn observed_executable_path(
    expected: &ProcessIdentity,
    deadline: std::time::Instant,
) -> std::io::Result<std::path::PathBuf> {
    let timely = || {
        if std::time::Instant::now() >= deadline {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "executable observation deadline",
            ))
        } else {
            Ok(())
        }
    };
    timely()?;
    if MacIdentity.identity(expected.pid()).as_ref() != Some(expected) {
        return Err(std::io::Error::other(
            "executable observation identity unavailable or changed",
        ));
    }
    timely()?;
    let pid = i32::try_from(expected.pid())
        .map_err(|_| std::io::Error::other("executable observation PID"))?;
    let path = libproc::proc_pid::pidpath(pid)
        .map_err(|_| std::io::Error::other("executable observation path unavailable"))?;
    timely()?;
    if MacIdentity.identity(expected.pid()).as_ref() != Some(expected) {
        return Err(std::io::Error::other(
            "executable observation identity unavailable or changed",
        ));
    }
    timely()?;
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(std::io::Error::other("executable observation invalid path"));
    }
    Ok(path)
}

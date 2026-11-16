use hf_application::process::IdentityPort;
use hf_domain::process::{IdentityObservation, ProcessIdentity};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
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

    fn observe(&self, expected: &ProcessIdentity) -> IdentityObservation {
        match self.identity(expected.pid()) {
            Some(actual) if actual == *expected => return IdentityObservation::Matching,
            Some(_) => return IdentityObservation::Mismatch,
            None => {}
        }
        // A failed identity lookup does not preserve errno or prove absence.
        // A positive PID and None perform only a presence probe, sending no signal.
        let Ok(native_pid) = i32::try_from(expected.pid()) else {
            return IdentityObservation::Unknown;
        };
        if native_pid <= 1 {
            return IdentityObservation::Unknown;
        }
        let before = boot_uuid();
        let probe = kill(Pid::from_raw(native_pid), None);
        let after = boot_uuid();
        classify_absence(expected, before, probe, after)
    }
}

fn classify_absence(
    expected: &ProcessIdentity,
    before: Option<String>,
    probe: Result<(), Errno>,
    after: Option<String>,
) -> IdentityObservation {
    let normalize = |boot| {
        ProcessIdentity::new(
            expected.pid(),
            expected.start_seconds(),
            expected.start_microseconds(),
            boot,
        )
        .ok()
        .map(|identity| identity.boot_uuid().to_owned())
    };
    let before = before.and_then(normalize);
    let after = after.and_then(normalize);
    if before.is_some() && before == after && probe == Err(Errno::ESRCH) {
        IdentityObservation::Dead
    } else {
        IdentityObservation::Unknown
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

#[cfg(test)]
mod tests {
    use super::classify_absence;
    use hf_domain::process::{IdentityObservation, ProcessIdentity};
    use nix::errno::Errno;

    const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";

    fn identity() -> ProcessIdentity {
        ProcessIdentity::new(42, 1_700_000_000, 5, BOOT.into()).unwrap()
    }

    #[test]
    fn stable_valid_boot_and_esrch_prove_dead() {
        let expected = identity();
        let upper = BOOT.to_ascii_uppercase();

        assert_eq!(
            classify_absence(&expected, Some(upper), Err(Errno::ESRCH), Some(BOOT.into())),
            IdentityObservation::Dead
        );
    }

    #[test]
    fn successful_and_non_esrch_probes_remain_unknown() {
        let expected = identity();
        for probe in [Ok(()), Err(Errno::EPERM), Err(Errno::EINVAL)] {
            assert_eq!(
                classify_absence(&expected, Some(BOOT.into()), probe, Some(BOOT.into())),
                IdentityObservation::Unknown
            );
        }
    }

    #[test]
    fn missing_malformed_or_changing_boot_evidence_remains_unknown() {
        let expected = identity();
        let cases = [
            (None, Some(BOOT.into())),
            (Some(BOOT.into()), None),
            (Some("malformed".into()), Some("malformed".into())),
            (
                Some(BOOT.into()),
                Some("11111111-2222-3333-4444-555555555555".into()),
            ),
        ];
        for (before, after) in cases {
            assert_eq!(
                classify_absence(&expected, before, Err(Errno::ESRCH), after),
                IdentityObservation::Unknown
            );
        }
    }
}

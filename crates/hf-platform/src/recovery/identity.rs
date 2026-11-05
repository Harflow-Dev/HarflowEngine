use super::timely;
use hf_application::recovery::{RecoveryError, RecoveryIdentityPort};
use hf_domain::{
    process::ProcessIdentity,
    recovery::{ProcessEvidence, ProcessState},
};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
use std::time::Instant;
use sysctl::{Ctl, Sysctl};

#[derive(Clone, Copy, Debug, Default)]
pub struct MacRecoveryIdentity;

fn boot_uuid(expected: &ProcessIdentity) -> Option<String> {
    let value = Ctl::new("kern.bootsessionuuid").ok()?.value_string().ok()?;
    ProcessIdentity::new(
        expected.pid(),
        expected.start_seconds(),
        expected.start_microseconds(),
        value,
    )
    .ok()
    .map(|identity| identity.boot_uuid().into())
}

impl RecoveryIdentityPort for MacRecoveryIdentity {
    fn observe(
        &self,
        expected: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<ProcessEvidence, RecoveryError> {
        timely(deadline)?;
        let before = boot_uuid(expected);
        let info = pidinfo::<BSDInfo>(expected.pid() as i32, 0);
        // pidinfo's String error does not preserve errno. Only a separate positive
        // ESRCH presence probe can establish absence; None sends no signal.
        let absent =
            info.is_err() && kill(Pid::from_raw(expected.pid() as i32), None) == Err(Errno::ESRCH);
        let after = boot_uuid(expected);
        timely(deadline)?;
        let mut evidence = ProcessEvidence {
            expected: expected.clone(),
            state: ProcessState::Unknown,
            actual: None,
            boot: None,
        };
        let Some(boot) = before.filter(|value| Some(value) == after.as_ref()) else {
            return Ok(evidence);
        };
        if boot != expected.boot_uuid() {
            evidence.state = ProcessState::BootChanged;
            evidence.boot = Some(boot);
            return Ok(evidence);
        }
        if let Ok(info) = info {
            qualify(&mut evidence, &info, boot);
        } else if absent {
            evidence.state = ProcessState::AbsentPid;
            evidence.boot = Some(boot);
        }
        Ok(evidence)
    }
}

fn qualify(evidence: &mut ProcessEvidence, info: &BSDInfo, boot: String) {
    if info.pbi_pid != evidence.expected.pid() {
        return;
    }
    let Ok(actual) = ProcessIdentity::new(
        info.pbi_pid,
        info.pbi_start_tvsec,
        info.pbi_start_tvusec,
        boot.clone(),
    ) else {
        return;
    };
    let state = if actual != evidence.expected {
        ProcessState::PidReused
    } else {
        match info.pbi_status {
            1..=4 => ProcessState::Present,
            5 => ProcessState::Zombie,
            _ => return,
        }
    };
    evidence.state = state;
    evidence.actual = Some(actual);
    evidence.boot = Some(boot);
}

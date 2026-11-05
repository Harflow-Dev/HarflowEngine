use super::RecoveryError;
use hf_domain::{
    capture::{CaptureSnapshot, FileIdentity},
    ownership::{Id, Phase},
    process::{ProcessExit, ProcessIdentity},
    recovery::*,
};
pub(super) fn record(record: &RecoveryRecord, id: &Id) -> Result<(), RecoveryError> {
    let own = &record.ownership;
    if own.request.operation != *id || own.request.generation != record.source.generation {
        return Err(RecoveryError::Stale);
    }
    if own.phase == Phase::Released {
        return Err(RecoveryError::Refused);
    }
    if let Some(c) = &record.capture
        && (own.phase != Phase::AcceptanceRecorded
            || own.cancelled
            || own.fence().as_ref() != Some(&c.binding.fence)
            || own.child.as_ref() != Some(&c.binding.child))
    {
        return Err(RecoveryError::Stale);
    }
    Ok(())
}
pub(super) fn process(
    e: &ProcessEvidence,
    expected: &ProcessIdentity,
) -> Result<(), RecoveryError> {
    let boot = e.boot.as_deref();
    let valid = e.expected == *expected
        && match e.state {
            ProcessState::Present | ProcessState::Zombie => {
                e.actual.as_ref() == Some(expected) && boot == Some(expected.boot_uuid())
            }
            ProcessState::AbsentPid => e.actual.is_none() && boot == Some(expected.boot_uuid()),
            ProcessState::PidReused => e.actual.as_ref().is_some_and(|a| {
                a.pid() == expected.pid()
                    && a != expected
                    && a.boot_uuid() == expected.boot_uuid()
                    && boot == Some(a.boot_uuid())
            }),
            ProcessState::BootChanged => {
                e.actual.is_none()
                    && boot.is_some_and(|b| valid_boot(b) && b != expected.boot_uuid())
            }
            ProcessState::Unknown => e.actual.is_none() && boot.is_none(),
        };
    if valid {
        Ok(())
    } else {
        Err(RecoveryError::Invalid)
    }
}
fn valid_boot(b: &str) -> bool {
    b.len() == 36
        && b.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
fn extent(raw: &StreamObservation, identity: &FileIdentity, offset: u64) -> Option<bool> {
    match raw {
        StreamObservation::Present {
            identity: observed,
            length,
        } if observed == identity && *length >= offset && *length <= 65536 => {
            Some(*length > offset)
        }
        _ => None,
    }
}
pub(super) fn assessment(
    c: Option<&CaptureSnapshot>,
    raw: Option<&RawEvidence>,
) -> CaptureAssessment {
    let Some(c) = c else {
        return CaptureAssessment::NotRecorded;
    };
    let Some(raw) = raw else {
        return CaptureAssessment::Unqualified;
    };
    let p = &c.progress;
    let a = extent(&raw.stdout, &c.binding.stdout, p.stdout);
    let b = extent(&raw.stderr, &c.binding.stderr, p.stderr);
    if a.is_none() || b.is_none() {
        return CaptureAssessment::Unqualified;
    }
    if a == Some(true) || b == Some(true) {
        return CaptureAssessment::UnindexedTail;
    }
    if c.exit != ProcessExit::Unobserved && p.stdout_eof && p.stderr_eof && !p.loss {
        CaptureAssessment::RecordedTerminalExtentObserved
    } else {
        CaptureAssessment::Incomplete
    }
}

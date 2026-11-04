use crate::{Error, Result};
use hf_domain::{ownership::*, process::ProcessIdentity};
use sha2::{Digest as ShaDigest, Sha256};
pub(crate) fn validate(request: &PreparedRequest) -> Result<()> {
    if request.payload.len() > 16 * 1024 {
        return Err(Error::LimitExceeded);
    }
    let actual = format!("{:x}", Sha256::digest(&request.payload));
    if actual != request.digest.as_str() {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
pub(crate) fn process(value: &ProcessIdentity) -> String {
    format!(
        "{}:{}:{}:{}",
        value.pid(),
        value.start_seconds(),
        value.start_microseconds(),
        value.boot_uuid()
    )
}
pub(crate) fn decode_process(value: &str) -> Result<ProcessIdentity> {
    let mut parts = value.split(':');
    let pid = parts
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::InvalidOwnership)?;
    let seconds = parts
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::InvalidOwnership)?;
    let micros = parts
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::InvalidOwnership)?;
    let boot = parts.next().ok_or(Error::InvalidOwnership)?;
    if parts.next().is_some() {
        return Err(Error::InvalidOwnership);
    }
    let result = ProcessIdentity::new(pid, seconds, micros, boot.to_owned())
        .map_err(|_| Error::InvalidOwnership)?;
    if process(&result) != value {
        return Err(Error::InvalidOwnership);
    }
    Ok(result)
}
pub(crate) fn id(value: String) -> Result<Id> {
    let result = Id::new(value.clone()).map_err(|_| Error::InvalidOwnership)?;
    if result.as_str() != value {
        return Err(Error::InvalidOwnership);
    }
    Ok(result)
}
pub(crate) fn phase(value: i64) -> Result<Phase> {
    match value {
        0 => Ok(Phase::Prepared),
        1 => Ok(Phase::Claimed),
        2 => Ok(Phase::SpawnObserved),
        3 => Ok(Phase::AcceptanceRecorded),
        4 => Ok(Phase::Released),
        _ => Err(Error::InvalidOwnership),
    }
}
pub(crate) fn phase_number(value: Phase) -> i64 {
    match value {
        Phase::Prepared => 0,
        Phase::Claimed => 1,
        Phase::SpawnObserved => 2,
        Phase::AcceptanceRecorded => 3,
        Phase::Released => 4,
    }
}
pub(crate) fn next(snapshot: &mut Snapshot) -> Result<()> {
    snapshot.revision = snapshot
        .revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(Error::InvalidOwnership)?;
    Ok(())
}

pub(crate) fn revision_valid(snapshot: &Snapshot) -> bool {
    let base = match snapshot.phase {
        Phase::Prepared => 0,
        Phase::Claimed => 1,
        Phase::SpawnObserved => 2,
        Phase::AcceptanceRecorded => 3,
        Phase::Released => return snapshot.revision >= 2,
    };
    snapshot.revision == base + u64::from(snapshot.cancelled)
}

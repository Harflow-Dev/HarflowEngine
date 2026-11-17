use super::{CallerLink, CallerObservation, CallerProcessRecord, InitAnchor, unknown};
use hf_domain::process::ProcessIdentity;
use std::{io, time::Instant};

/// Injectable observations for the same bounded algorithm used by production.
/// Provider calls are not a claim about the number of underlying kernel calls.
pub trait CallerEvidenceSource {
    fn now(&mut self) -> Instant;
    fn boot_uuid(&mut self) -> io::Result<String>;
    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord>;
    fn real_uid(&mut self) -> u32;
}
fn timely<S: CallerEvidenceSource + ?Sized>(source: &mut S, deadline: Instant) -> io::Result<()> {
    if source.now() >= deadline {
        Err(unknown())
    } else {
        Ok(())
    }
}
fn read<S: CallerEvidenceSource + ?Sized, T>(
    source: &mut S,
    deadline: Instant,
    operation: impl FnOnce(&mut S) -> io::Result<T>,
) -> io::Result<T> {
    timely(source, deadline)?;
    let result = operation(source);
    timely(source, deadline)?;
    result
}
fn record<S: CallerEvidenceSource + ?Sized>(
    source: &mut S,
    pid: u32,
    deadline: Instant,
) -> io::Result<(CallerProcessRecord, String)> {
    let before = read(source, deadline, |s| s.boot_uuid())?;
    let item = read(source, deadline, |s| s.record(pid))?.validate(pid)?;
    let after = read(source, deadline, |s| s.boot_uuid())?;
    if before != after {
        return Err(unknown());
    }
    Ok((item, before))
}
fn init<S: CallerEvidenceSource + ?Sized>(
    source: &mut S,
    deadline: Instant,
) -> io::Result<InitAnchor> {
    let (item, boot_uuid) = record(source, 1, deadline)?;
    if item.uid != 0 {
        return Err(unknown());
    }
    Ok(InitAnchor {
        start_seconds: item.start_seconds,
        start_microseconds: item.start_microseconds,
        boot_uuid,
        uid: item.uid,
    })
}
fn link<S: CallerEvidenceSource + ?Sized>(
    source: &mut S,
    pid: u32,
    deadline: Instant,
) -> io::Result<CallerLink> {
    let (item, boot) = record(source, pid, deadline)?;
    let identity = ProcessIdentity::new(pid, item.start_seconds, item.start_microseconds, boot)
        .map_err(|_| unknown())?;
    Ok(CallerLink {
        identity,
        parent: item.parent,
        uid: item.uid,
    })
}
/// Observe and exactly re-observe every non-init edge and the distinct init anchor.
/// At N non-init links, at most 2N+2 record and 4N+4 boot provider calls occur.
pub fn observe_caller_with_source<S: CallerEvidenceSource + ?Sized>(
    pid: u32,
    deadline: Instant,
    source: &mut S,
) -> io::Result<CallerObservation> {
    if pid <= 1 || i32::try_from(pid).is_err() {
        return Err(unknown());
    }
    let mut chain: Vec<CallerLink> = Vec::new();
    let mut current = pid;
    let init = init(source, deadline)?;
    while current != 1 {
        if chain.len() >= 128 || chain.iter().any(|v| v.identity.pid() == current) {
            return Err(unknown());
        }
        let item = link(source, current, deadline)?;
        if item.identity.boot_uuid() != init.boot_uuid.to_ascii_lowercase() {
            return Err(unknown());
        }
        current = item.parent;
        chain.push(item);
    }
    if chain.first().is_none_or(|v| v.uid != source.real_uid()) {
        return Err(unknown());
    }
    for item in &chain {
        if link(source, item.identity.pid(), deadline)? != *item {
            return Err(unknown());
        }
    }
    if self::init(source, deadline)? != init {
        return Err(unknown());
    }
    Ok(CallerObservation { chain, init })
}

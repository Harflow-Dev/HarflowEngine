//! Bounded same-UID caller observations, never human credentials.
use hf_domain::process::ProcessIdentity;
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use nix::sys::socket::{audit_token_t, getsockopt, sockopt};
use std::{io, os::unix::net::UnixStream, time::Instant};
use sysctl::{Ctl, Sysctl};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerLink {
    pub identity: ProcessIdentity,
    pub parent: u32,
    pub uid: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerObservation {
    /// Includes the actual caller itself and every non-init ancestor.
    pub chain: Vec<CallerLink>,
    pub init: InitAnchor,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitAnchor {
    pub start_seconds: u64,
    pub start_microseconds: u64,
    pub boot_uuid: String,
    pub uid: u32,
}
fn init_anchor(deadline: Instant) -> io::Result<InitAnchor> {
    if Instant::now() >= deadline {
        return Err(unknown());
    }
    let boot = Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value_string())
        .map_err(|_| unknown())?;
    let info = pidinfo::<BSDInfo>(1, 0).map_err(|_| unknown())?;
    let after = Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value_string())
        .map_err(|_| unknown())?;
    if boot != after
        || info.pbi_pid != 1
        || info.pbi_ppid != 0
        || info.pbi_uid != 0
        || !(1..=4).contains(&info.pbi_status)
        || info.pbi_start_tvsec == 0
        || info.pbi_start_tvusec >= 1_000_000
        || Instant::now() >= deadline
    {
        return Err(unknown());
    }
    Ok(InitAnchor {
        start_seconds: info.pbi_start_tvsec,
        start_microseconds: info.pbi_start_tvusec,
        boot_uuid: boot,
        uid: info.pbi_uid,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerGeneration {
    /// The safe available APIs do not correlate opaque audit-token generation to BSDInfo.
    Uncorrelated,
}
/// Kernel socket credentials plus a separately bookended process observation.
/// This is not standalone mutation admission: generation correlation remains Unknown.
pub struct AuthenticatedPeer {
    pub uid: u32,
    pub pid: u32,
    /// Opaque kernel evidence. No undocumented token bytes are decoded.
    pub token: audit_token_t,
    pub observation: CallerObservation,
    pub generation: PeerGeneration,
}
fn unknown() -> io::Error {
    io::Error::other("caller observation incomplete or changed")
}
fn link(pid: u32, deadline: Instant) -> io::Result<CallerLink> {
    if Instant::now() >= deadline {
        return Err(unknown());
    }
    let native = i32::try_from(pid).map_err(|_| unknown())?;
    let boot = Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value_string())
        .map_err(|_| unknown())?;
    let info = pidinfo::<BSDInfo>(native, 0).map_err(|_| unknown())?;
    let after = Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value_string())
        .map_err(|_| unknown())?;
    let before = ProcessIdentity::new(
        pid,
        info.pbi_start_tvsec,
        info.pbi_start_tvusec,
        boot.clone(),
    )
    .map_err(|_| unknown())?;
    if boot != after
        || info.pbi_pid != pid
        || !(1..=4).contains(&info.pbi_status)
        || info.pbi_start_tvsec != before.start_seconds()
        || info.pbi_start_tvusec != before.start_microseconds()
        || Instant::now() >= deadline
    {
        return Err(unknown());
    }
    Ok(CallerLink {
        identity: before,
        parent: info.pbi_ppid,
        uid: info.pbi_uid,
    })
}
/// Requires the actual caller UID only. Ancestors may belong to root.
/// Every edge is re-observed; a truncated or reparented walk is Unknown.
pub fn observe_caller(pid: u32, deadline: Instant) -> io::Result<CallerObservation> {
    let mut chain = Vec::new();
    let mut current = pid;
    let init = init_anchor(deadline)?;
    while current != 1 {
        if chain.len() >= 128
            || chain
                .iter()
                .any(|v: &CallerLink| v.identity.pid() == current)
        {
            return Err(unknown());
        }
        let item = link(current, deadline)?;
        if item.identity.boot_uuid() != init.boot_uuid.to_ascii_lowercase() {
            return Err(unknown());
        }
        current = item.parent;
        chain.push(item);
        if current == 0 {
            return Err(unknown());
        }
    }
    if chain
        .first()
        .is_none_or(|v| v.uid != nix::unistd::getuid().as_raw())
    {
        return Err(unknown());
    }
    for item in &chain {
        if link(item.identity.pid(), deadline)? != *item {
            return Err(unknown());
        }
    }
    if init_anchor(deadline)? != init {
        return Err(unknown());
    }
    Ok(CallerObservation { chain, init })
}
pub fn current_caller(deadline: Instant) -> io::Result<CallerObservation> {
    observe_caller(std::process::id(), deadline)
}
pub fn authenticate_peer(stream: &UnixStream, deadline: Instant) -> io::Result<AuthenticatedPeer> {
    let credentials = getsockopt(stream, sockopt::LocalPeerCred)?;
    let native = getsockopt(stream, sockopt::LocalPeerPid)?;
    let token = getsockopt(stream, sockopt::LocalPeerToken)?;
    let pid = u32::try_from(native).map_err(|_| unknown())?;
    let uid = credentials.uid();
    if uid != nix::unistd::getuid().as_raw() {
        return Err(unknown());
    }
    let observation = observe_caller(pid, deadline)?;
    if observation.chain.first().is_none_or(|v| v.uid != uid)
        || getsockopt(stream, sockopt::LocalPeerPid)? != native
        || getsockopt(stream, sockopt::LocalPeerCred)?.uid() != uid
        || getsockopt(stream, sockopt::LocalPeerToken)? != token
    {
        return Err(unknown());
    }
    Ok(AuthenticatedPeer {
        uid,
        pid,
        token,
        observation,
        generation: PeerGeneration::Uncorrelated,
    })
}

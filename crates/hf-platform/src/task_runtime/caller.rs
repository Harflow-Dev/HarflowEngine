//! Bounded same-UID caller observations, never human credentials.
use hf_domain::process::ProcessIdentity;
use nix::sys::socket::{audit_token_t, getsockopt, sockopt};
use std::{io, os::unix::net::UnixStream, time::Instant};
mod kernel_reader;
mod observe;
mod record;
mod system;
pub use observe::{CallerEvidenceSource, observe_caller_with_source};
pub use record::{CallerProcessRecord, CallerRecordLayout, decode_caller_record};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerGeneration {
    /// The available APIs do not correlate opaque audit-token generation to a process record.
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
/// Requires the actual caller UID only; ancestors may belong to root.
pub fn observe_caller(pid: u32, deadline: Instant) -> io::Result<CallerObservation> {
    observe_caller_with_source(pid, deadline, &mut system::SystemSource)
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

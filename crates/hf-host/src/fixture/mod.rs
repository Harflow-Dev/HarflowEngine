//! Source-controlled single-threaded fixed fixture composition, never a runtime launcher.
mod capture;
mod effect;
mod owned;
mod owner;
mod parent;
mod store;
#[cfg(test)]
pub(crate) mod test_support;
use hf_application::process::IdentityPort;
use hf_domain::{fixture::*, ownership::Id, process::ProcessIdentity};
use hf_interfaces::fixture::{decode, encode};
use hf_platform::{
    fixture::{FixtureGate, PrivateIo, check_entry},
    process::{
        MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome,
    },
};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const PARENT: &str = "--internal-parent-fixture-v1";
const OWNER: &str = "--internal-owner-fixture-v1";
const SATURATION: &str = "--internal-effect-saturation-fixture-v1";
const EFFECT: &str = "--internal-effect-fixture-v1";
pub(super) fn run(mode: &str) -> io::Result<()> {
    check_entry()?;
    let budget = match mode {
        PARENT => 10,
        OWNER => 8,
        EFFECT | SATURATION => 4,
        _ => return Err(invalid()),
    };
    let deadline = Instant::now() + Duration::from_secs(budget);
    let channel = PrivateIo::stdio(deadline);
    match mode {
        PARENT => parent::run(channel, deadline),
        OWNER => owner::run(channel, deadline),
        EFFECT | SATURATION => effect::run(channel, deadline, mode == SATURATION),
        _ => Err(invalid()),
    }
}
fn invalid() -> io::Error {
    io::Error::other("private fixture refused")
}
fn map<T, E>(value: Result<T, E>) -> io::Result<T> {
    value.map_err(|_| invalid())
}
fn receive(channel: &PrivateIo) -> io::Result<Frame> {
    map(decode(&channel.read_packet()?))
}
fn send(channel: &PrivateIo, frame: &Frame) -> io::Result<()> {
    channel.write_packet(&map(encode(frame))?)
}
fn child_send(child: &mut RetainedOwnedChild, frame: &Frame, until: Instant) -> io::Result<()> {
    child.write_fixture_frame(&map(encode(frame))?, until)
}
fn child_receive(child: &mut RetainedOwnedChild, until: Instant) -> io::Result<Frame> {
    map(decode(&child.read_fixture_frame(until)?))
}
fn self_identity() -> io::Result<ProcessIdentity> {
    MacIdentity.identity(std::process::id()).ok_or_else(invalid)
}
fn spawn(mode: &str, cwd: &Path, until: Instant) -> io::Result<RetainedOwnedChild> {
    spawn_with(mode, cwd, &MacIdentity, until)
}
fn spawn_with(
    mode: &str,
    cwd: &Path,
    identity: &impl IdentityPort,
    until: Instant,
) -> io::Result<RetainedOwnedChild> {
    // Entry already qualified ambient descriptors. The safe spawn seam now
    // owns inheritance; coordinator sentinel descriptors intentionally stay open.
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: std::env::current_exe()?,
            cwd: cwd.to_path_buf(),
            args: vec![mode.into()],
            environment: Vec::new(),
            capture_limit: 24576,
        },
        identity,
        until,
    )
}
fn ready(child: &RetainedOwnedChild) -> io::Result<ProcessIdentity> {
    if child.initialization() != SpawnInitialization::Ready {
        return Err(invalid());
    }
    child.registered_identity().cloned().ok_or_else(invalid)
}
fn root_parent(root: &str) -> io::Result<PathBuf> {
    let path = Path::new(root);
    if path.file_name().and_then(|v| v.to_str()) != Some("store") {
        return Err(invalid());
    }
    path.parent().map(Path::to_path_buf).ok_or_else(invalid)
}
/// Only fresh private modes call this before Store handles or worker threads.
fn bootstrap_cwd(cwd: &Path) -> io::Result<()> {
    let physical = std::fs::canonicalize(cwd)?;
    if !physical.is_dir() || physical != cwd {
        return Err(invalid());
    }
    std::env::set_current_dir(physical)
}
fn gate_config(root: &str, gate: &Option<Gate>) -> io::Result<FixtureGate> {
    let parent = root_parent(root)?;
    if let Some(gate) = gate
        && Path::new(&gate.path).parent() != Some(parent.as_path())
    {
        return Err(invalid());
    }
    Ok(FixtureGate::new(gate.clone()))
}
/// One cleanup path after every spawn. Closing input is cooperative; a signal
/// requires the original registered direct child plus a fresh matching identity.
/// Failure retains uncertainty; it never releases an ownership reservation.
fn cleanup(child: &mut RetainedOwnedChild) -> io::Result<()> {
    child.close_stdin();
    let until = Instant::now() + Duration::from_secs(2);
    if matches!(child.wait_until(until, || false)?, WaitOutcome::Exited(_)) {
        return Ok(());
    }
    let Some(identity) = child.registered_identity().cloned() else {
        return Err(invalid());
    };
    for signal in [OwnedSignal::Terminate, OwnedSignal::Kill] {
        let until = Instant::now() + Duration::from_secs(2);
        let _ = child.request_signal(&identity, &MacIdentity, signal, until)?;
        if matches!(child.wait_until(until, || false)?, WaitOutcome::Exited(_)) {
            return Ok(());
        }
    }
    Err(invalid())
}
fn report(operation: Id, generation: Id, kind: ReportKind) -> Frame {
    Frame::Report {
        operation,
        generation,
        kind,
    }
}

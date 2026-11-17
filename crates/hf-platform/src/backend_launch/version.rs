use super::{identity, timely};
use crate::process::{MacIdentity, RetainedOwnedChild, SpawnInitialization, SpawnSpec};
use hf_application::prepared_launch::StreamingDigestFactory;
use hf_domain::{prepared_launch::BinaryPin, process::ProcessExit, task_launch::LaunchRefusal};
use std::{
    ffi::OsString,
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn probe(
    expected: &BinaryPin,
    environment: &[(OsString, OsString)],
    factory: &impl StreamingDigestFactory,
    pending: &mut Option<RetainedOwnedChild>,
    deadline: Instant,
) -> Result<String, LaunchRefusal> {
    let spec = prepare_version_probe(expected, environment, factory, deadline)?;
    // The safe spawn seam performs its own before/after metadata checks. The
    // expected preparation pin was just rechecked, rather than accepting a fresh
    // replacement as the probe identity. Path-based exec still has a remaining
    // check/exec race; this does not claim atomic exec-from-open-file authority.
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline)
        .map_err(|_| LaunchRefusal::Unavailable)?;
    child.close_stdin();
    *pending = Some(child);
    collect(pending, deadline)
}
/// Builds a version-only spawn specification after rechecking the original
/// preparation pin. This function never executes the candidate.
pub fn prepare_version_probe(
    expected: &BinaryPin,
    environment: &[(OsString, OsString)],
    factory: &impl StreamingDigestFactory,
    deadline: Instant,
) -> Result<SpawnSpec, LaunchRefusal> {
    let spec = SpawnSpec {
        executable: expected.physical_path.clone().into(),
        cwd: std::env::current_dir().map_err(|_| LaunchRefusal::Unavailable)?,
        args: vec!["--version".into()],
        environment: environment.into(),
        capture_limit: 4096,
    };
    timely(deadline)?;
    let physical = Path::new(&expected.physical_path);
    if std::fs::canonicalize(&expected.logical_path)
        .ok()
        .as_deref()
        != Some(physical)
    {
        return Err(LaunchRefusal::IdentityChanged);
    }
    let mut actual = identity::observe(physical, factory, deadline)?;
    actual.logical_path = expected.logical_path.clone();
    if actual != *expected {
        return Err(LaunchRefusal::IdentityChanged);
    }
    timely(deadline)?;
    Ok(spec)
}
fn collect(
    pending: &mut Option<RetainedOwnedChild>,
    deadline: Instant,
) -> Result<String, LaunchRefusal> {
    let child = pending.as_mut().ok_or(LaunchRefusal::Unavailable)?;
    if child.initialization() != SpawnInitialization::Ready {
        return Err(LaunchRefusal::ProbeUnsettled);
    }
    loop {
        if Instant::now() >= deadline {
            return Err(LaunchRefusal::ProbeUnsettled);
        }
        child
            .pump(deadline, 8192)
            .map_err(|_| LaunchRefusal::ProbeUnsettled)?;
        child
            .try_wait()
            .map_err(|_| LaunchRefusal::ProbeUnsettled)?;
        let capture = child.capture();
        if child.exit() != ProcessExit::Unobserved && capture.stdout.eof && capture.stderr.eof {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let capture = child.capture();
    let result = if child.exit() == ProcessExit::Code(0)
        && !capture.stdout.overflow
        && !capture.stderr.overflow
        && capture.stdout.error.is_none()
        && capture.stderr.error.is_none()
        && capture.stderr.bytes.is_empty()
    {
        parse_version(&capture.stdout.bytes)
    } else {
        Err(LaunchRefusal::Unavailable)
    };
    *pending = None;
    result
}
pub(super) fn parse_version(bytes: &[u8]) -> Result<String, LaunchRefusal> {
    let text = std::str::from_utf8(bytes).map_err(|_| LaunchRefusal::UnsupportedVersion)?;
    let version = text
        .strip_suffix(" (Claude Code)\n")
        .ok_or(LaunchRefusal::UnsupportedVersion)?;
    if version.len() > 32
        || version.split('.').count() != 3
        || !version
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(LaunchRefusal::UnsupportedVersion);
    }
    Ok(version.into())
}

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnInitialization, WaitOutcome},
    qualification::FixtureRoot,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[path = "child.rs"]
mod child;
#[path = "identity.rs"]
mod identity;
use child::{ChildGuard, settle_or_abort};
#[path = "canary.rs"]
mod canary;
#[path = "invocation.rs"]
mod invocation;
#[path = "root.rs"]
mod root;
#[path = "server.rs"]
mod server;
use root::SettledRoot;
#[path = "measurement.rs"]
mod measurement;
use measurement::{HELPER, PYTHON};

/// Proves the route in-process before the first Claude invocation.
pub(super) fn vendor_measurement_preflight() {
    measurement::run(measurement::MeasurementMode::FullFixture);
}

pub(super) fn vendor_version_only_diagnostic() {
    measurement::run(measurement::MeasurementMode::VersionOnlySysctl);
}

pub(super) fn vendor_pagesize_version_only_diagnostic() {
    measurement::run(measurement::MeasurementMode::PageSizeVersionOnly);
}

fn launch_preflight(
    root: &FixtureRoot,
    profile: &str,
    canary: &Path,
    positive: &Path,
    allowed_port: u16,
    denied_port: u16,
    nonce: &str,
) -> String {
    let args = [
        "-I".into(),
        "-S".into(),
        HELPER.into(),
        canary.as_os_str().to_owned(),
        positive.as_os_str().to_owned(),
        allowed_port.to_string().into(),
        denied_port.to_string().into(),
        nonce.into(),
    ];
    let spec = root
        .spawn_spec(
            profile,
            Path::new(PYTHON),
            &args,
            invocation::preflight_environment(root),
            4096,
        )
        .unwrap();
    let mut child = ChildGuard::new(
        RetainedOwnedChild::spawn(spec, &MacIdentity, deadline(3000)).unwrap(),
        "vendor-nofork-read-network-preflight",
    );
    if child.initialization() != SpawnInitialization::Ready {
        settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    let exit = child.wait_until(deadline(10_000));
    drain(&mut child);
    child.settle();
    let (stdout, stderr) = child.take_capture_bytes();
    let identity = child.identity_summary();
    assert_eq!(
        exit,
        ProcessExit::Code(0),
        "phase=vendor-nofork-read-network-preflight child identity={identity}; unsupported preflight helper exit; stdout={} stderr={}",
        bounded_summary(&stdout),
        bounded_summary(&stderr)
    );
    assert!(
        stderr.is_empty(),
        "preflight helper stderr is forbidden: {}",
        bounded_summary(&stderr)
    );
    String::from_utf8(stdout).unwrap().trim().to_owned()
}

fn launch_spawn_control(root: &FixtureRoot, profile: &str) -> String {
    let spec = root
        .spawn_spec(
            profile,
            Path::new(PYTHON),
            &[
                "-I".into(),
                "-S".into(),
                HELPER.into(),
                "--spawn-control".into(),
            ],
            invocation::preflight_environment(root),
            4096,
        )
        .unwrap();
    let mut child = ChildGuard::new(
        RetainedOwnedChild::spawn(spec, &MacIdentity, deadline(3000)).unwrap(),
        "controlled-posix-spawn-positive",
    );
    if child.initialization() != SpawnInitialization::Ready {
        settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    let exit = child.wait_until(deadline(5000));
    drain(&mut child);
    child.settle();
    let (stdout, stderr) = child.take_capture_bytes();
    let identity = child.identity_summary();
    assert_eq!(
        exit,
        ProcessExit::Code(0),
        "phase=controlled-posix-spawn-positive child identity={identity}; controlled posix_spawn positive control failed; stdout={} stderr={}",
        bounded_summary(&stdout),
        bounded_summary(&stderr)
    );
    assert!(
        stderr.is_empty(),
        "controlled posix_spawn stderr: {}",
        bounded_summary(&stderr)
    );
    String::from_utf8(stdout).unwrap().trim().to_owned()
}

pub(super) fn bounded_summary(bytes: &[u8]) -> String {
    const LIMIT: usize = 1024;
    let shown = String::from_utf8_lossy(&bytes[..bytes.len().min(LIMIT)]);
    let suffix = if bytes.len() > LIMIT {
        " [truncated]"
    } else {
        ""
    };
    format!("{shown:?}{suffix}")
}

fn drain(child: &mut RetainedOwnedChild) {
    let end = deadline(1500);
    while (!child.capture().stdout.eof || !child.capture().stderr.eof) && Instant::now() < end {
        child.pump(end, 8192).unwrap();
    }
    assert!(
        child.capture().stdout.eof && child.capture().stderr.eof,
        "settled preflight streams reached EOF"
    );
}

fn deadline(milliseconds: u64) -> Instant {
    Instant::now() + Duration::from_millis(milliseconds)
}

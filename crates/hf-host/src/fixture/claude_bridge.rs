//! Closed source-owned no-fork diagnostic bridge. LaunchReady is not exec success.
use super::*;
use hf_domain::claude_qualification::{self as cq, ClaudeQualification};
use hf_platform::fixture::qualify_ledger;
use hf_storage::{Limits, RootHold, Sha256Digest, Store};
use std::{os::unix::process::CommandExt, process::Command};

pub(crate) fn validate(value: &ClaudeQualification, until: Instant) -> io::Result<()> {
    map(hf_backends::claude_qualification::validate_command(value))?;
    let expected =
        hf_platform::qualification::recorded_vendor_profile(&value.topology, value.port)?;
    if expected.as_bytes() != value.profile
        || Path::new(&value.cwd) != Path::new(&value.topology.root.path)
        || std::env::current_dir()? != Path::new(&value.cwd)
        || value.vendor_version != cq::VENDOR_VERSION
        || value.vendor_binary.physical_path != cq::VENDOR_PATH
        || value.vendor_binary.logical_path != cq::VENDOR_PATH
        || value.vendor_binary.metadata.length != cq::VENDOR_LENGTH
        || value.vendor_binary.digest.as_str() != cq::VENDOR_DIGEST
    {
        return Err(invalid());
    }
    let gate = map(hf_platform::prepared_launch::observe_binary(
        &std::env::current_exe()?,
        &Sha256Digest,
        until,
    ))?;
    let vendor = map(hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(cq::VENDOR_PATH),
        &Sha256Digest,
        until,
    ))?;
    if gate != value.gate_binary || vendor != value.vendor_binary {
        return Err(invalid());
    }
    hf_platform::qualification::validate_recorded_topology(&value.topology)?;
    let root = Path::new(&value.cwd).join("store");
    let hold = map(RootHold::open(&root, until, hf_platform::lease::shared()))?;
    if hold.source_stamp() != &value.source || qualify_ledger(&root)? != value.ledger {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn run(channel: PrivateIo, until: Instant) -> io::Result<()> {
    let Frame::Start(start) = receive(&channel)? else {
        return Err(invalid());
    };
    channel.expect_eof()?;
    let actual = self_identity()?;
    let (value, request_digest) = observe(&start, &actual, until)?;
    validate(&value, until)?;
    let (current, current_digest) = observe(&start, &actual, until)?;
    if current != value || current_digest != request_digest {
        return Err(invalid());
    }
    // All Store/root guards are gone. Dormant CLOEXEC sentinels remain owned.
    {
        let _barrier = hf_platform::lease::exec_barrier()?;
    }
    send(
        &channel,
        &Frame::LaunchReady {
            operation: start.operation.clone(),
            attempt: start.fence.attempt.clone(),
            request_digest,
            child: actual,
        },
    )?;
    hf_platform::lease::restore_blocking_stdio()?;
    let mut command = command(&value)?;
    let _barrier = hf_platform::lease::exec_barrier()?;
    if Instant::now() >= until {
        return Err(invalid());
    }
    Err(command.exec())
}
fn observe(
    start: &EffectStart,
    actual: &ProcessIdentity,
    until: Instant,
) -> io::Result<(ClaudeQualification, hf_domain::ownership::Digest)> {
    let parent = root_parent(&start.root)?;
    if qualify_ledger(Path::new(&start.root))? != start.ledger
        || hf_platform::process::current_parent_identity().as_ref()
            != Some(&start.fence.owner.process)
    {
        return Err(invalid());
    }
    let hold = map(RootHold::open(
        &start.root,
        until,
        hf_platform::lease::shared(),
    ))?;
    let store = map(Store::open_with(
        &start.root,
        Limits::default(),
        until,
        hf_platform::lease::shared(),
        crate::durability::suppressed(),
    ))?;
    let stored = map(store.ownership_observe(&start.operation, until))?.ok_or_else(invalid)?;
    if stored.phase != hf_domain::ownership::Phase::SpawnObserved
        || stored.cancelled
        || stored.fence().as_ref() != Some(&start.fence)
        || stored.child.as_ref() != Some(actual)
        || start.operation != start.fence.operation
    {
        return Err(invalid());
    }
    let value = map(hf_interfaces::claude_qualification::decode(
        &stored.request,
        &Sha256Digest,
    ))?;
    if value.source != *hold.source_stamp()
        || value.ledger != start.ledger
        || Path::new(&value.cwd) != parent
        || Path::new(&value.topology.root.path) != parent
    {
        return Err(invalid());
    }
    Ok((value, stored.request.digest))
}
fn command(value: &ClaudeQualification) -> io::Result<Command> {
    map(hf_backends::claude_qualification::validate_command(value))?;
    let profile = std::str::from_utf8(&value.profile).map_err(|_| invalid())?;
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .env_clear()
        .args(["-p", profile, "/usr/bin/env", "-C"])
        .arg(Path::new(&value.cwd).join("workspace"))
        .arg(cq::VENDOR_PATH)
        .args(&value.command.args);
    for (key, value) in &value.command.environment {
        command.env(key, value);
    }
    Ok(command)
}

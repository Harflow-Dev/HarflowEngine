#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_interfaces::backend_measurement::{ClaudeEvent, decode_claude_event};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnInitialization, WaitOutcome},
    qualification::FixtureRoot,
};
use std::{ffi::OsString, path::Path};

pub(super) struct RunResult {
    pub(super) exit: ProcessExit,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) identity: String,
}

pub(super) fn run(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    args: &[OsString],
    env: Vec<(OsString, OsString)>,
    milliseconds: u64,
    phase: &'static str,
) -> RunResult {
    let capture_limit = if phase == "claude-version" {
        8192
    } else {
        1_048_576
    };
    let spec = root
        .spawn_spec(profile, Path::new(executable), args, env, capture_limit)
        .unwrap();
    let mut child = super::super::ChildGuard::new(
        RetainedOwnedChild::spawn(spec, &MacIdentity, super::super::deadline(3000)).unwrap(),
        phase,
    );
    if child.initialization() != SpawnInitialization::Ready {
        super::super::settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    let exit = child.wait_until(super::super::deadline(milliseconds));
    super::super::drain(&mut child);
    child.settle();
    let identity = child.identity_summary();
    let (stdout, stderr) = child.take_capture_bytes();
    RunResult {
        exit,
        stdout,
        stderr,
        identity,
    }
}

pub(super) fn require_protocol_witnesses(raw: &[u8], nonce: &str) {
    let mut text = false;
    let mut terminal = false;
    for line in raw
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        match decode_claude_event(line, nonce).expect("bounded Claude stream JSON event") {
            ClaudeEvent::TextWitness => text = true,
            ClaudeEvent::TerminalSuccess if !terminal => terminal = true,
            ClaudeEvent::TerminalSuccess | ClaudeEvent::TerminalFailure => {
                panic!("duplicate or failed Claude terminal event")
            }
            ClaudeEvent::Other => (),
        }
    }
    assert!(
        text && terminal,
        "text marker and terminal success must both be observed"
    );
}

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::owner_runtime::DigestPort;
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
    pub(super) stdout_eof: bool,
    pub(super) stderr_eof: bool,
    pub(super) stdout_overflow: bool,
    pub(super) stderr_overflow: bool,
    pub(super) stdout_error: Option<String>,
    pub(super) stderr_error: Option<String>,
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
    child.close_stdin();
    eprintln!(
        "phase={phase} child-ready identity={}",
        child.identity_summary()
    );
    let exit = child.wait_until(super::super::deadline(milliseconds));
    super::super::drain(&mut child);
    child.settle();
    let identity = child.identity_summary();
    let stdout_eof = child.capture().stdout.eof;
    let stderr_eof = child.capture().stderr.eof;
    let stdout_overflow = child.capture().stdout.overflow;
    let stderr_overflow = child.capture().stderr.overflow;
    let stdout_error = child.capture().stdout.error.clone();
    let stderr_error = child.capture().stderr.error.clone();
    let (stdout, stderr) = child.take_capture_bytes();
    eprintln!(
        "phase={phase} settled child identity={identity} exit={exit:?} stdout-eof={stdout_eof} stderr-eof={stderr_eof} stdout-overflow={stdout_overflow} stderr-overflow={stderr_overflow} stdout-error={stdout_error:?} stderr-error={stderr_error:?} stdout={} stderr={}",
        super::super::bounded_summary(&stdout),
        super::super::bounded_summary(&stderr)
    );
    for (stream, bytes) in [("stdout", &stdout), ("stderr", &stderr)] {
        let digest = hf_storage::Sha256Digest.digest_parts(&[bytes]);
        eprintln!(
            "phase={phase} stream={stream} raw-length={} raw-sha256={}",
            bytes.len(),
            digest.as_str()
        );
    }
    RunResult {
        exit,
        stdout,
        stderr,
        identity,
        stdout_eof,
        stderr_eof,
        stdout_overflow,
        stderr_overflow,
        stdout_error,
        stderr_error,
    }
}

impl RunResult {
    pub(super) fn capture_is_complete(&self) -> bool {
        self.stdout_eof
            && self.stderr_eof
            && !self.stdout_overflow
            && !self.stderr_overflow
            && self.stdout_error.is_none()
            && self.stderr_error.is_none()
    }
}

pub(super) fn protocol_witnesses(raw: &[u8], nonce: &str) -> Result<(), &'static str> {
    let mut text = false;
    let mut terminal = false;
    for line in raw
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if terminal {
            return Err("event after terminal success");
        }
        match decode_claude_event(line, nonce).map_err(|_| "invalid Claude stream event")? {
            ClaudeEvent::TextWitness if text => return Err("duplicate text witness"),
            ClaudeEvent::TextWitness => text = true,
            ClaudeEvent::TerminalSuccess if text => terminal = true,
            ClaudeEvent::TerminalSuccess => return Err("terminal success before text witness"),
            ClaudeEvent::TerminalFailure => return Err("failed Claude terminal event"),
            ClaudeEvent::Other => (),
        }
    }
    if text && terminal {
        Ok(())
    } else {
        Err("missing text or terminal success")
    }
}

pub(super) fn require_protocol_witnesses(raw: &[u8], nonce: &str) {
    protocol_witnesses(raw, nonce).expect("ordered Claude protocol witnesses");
}

#[cfg(test)]
#[path = "runner/protocol_tests.rs"]
mod protocol_tests;

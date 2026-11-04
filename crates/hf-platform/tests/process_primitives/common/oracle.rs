#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{FIXTURE, PYTHON, deadline};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, poll};
use std::io::{self, Read};
use std::ops::{Deref, DerefMut};
use std::os::fd::AsFd;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Instant;

pub struct BoundedOutput {
    pub bytes: Vec<u8>,
    pub status: ExitStatus,
}
pub(super) struct OracleGuard(pub(super) Child);
impl Deref for OracleGuard {
    type Target = Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for OracleGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OracleGuard {
    fn drop(&mut self) {
        self.0.stdin.take();
        let mut end = deadline(1_000);
        let mut sent_kill = false;
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= end && !sent_kill => {
                    let _ = self.0.kill();
                    sent_kill = true;
                    end = deadline(500);
                }
                Ok(None) if Instant::now() >= end => {
                    eprintln!("oracle helper cleanup failed for owned pid {}", self.0.id());
                    std::process::abort();
                }
                Ok(None) => (),
                Err(error) => {
                    eprintln!(
                        "oracle helper exit is unobserved for owned pid {}: {error}",
                        self.0.id()
                    );
                    std::process::abort();
                }
            }
            let wait = end
                .saturating_duration_since(Instant::now())
                .as_millis()
                .clamp(1, 20) as u16;
            let mut no_fds: [PollFd<'_>; 0] = [];
            if let Err(error) = poll(&mut no_fds, wait) {
                eprintln!("oracle helper cleanup poll failed: {error}");
                std::process::abort();
            }
        }
    }
}

pub fn run_bounded(program: &str, args: &[&str]) -> io::Result<Vec<u8>> {
    let result = run_bounded_until(program, args, deadline(3_000))?;
    if result.status.success() {
        Ok(result.bytes)
    } else {
        Err(io::Error::other(format!(
            "owned oracle helper exited with {}",
            result.status
        )))
    }
}

pub fn run_bounded_until(program: &str, args: &[&str], end: Instant) -> io::Result<BoundedOutput> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut child = OracleGuard(child);
    let Some(mut output) = child.stdout.take() else {
        return Err(io::Error::other("helper stdout absent"));
    };
    let flags = fcntl(&output, FcntlArg::F_GETFL).map_err(io::Error::from)?;
    fcntl(
        &output,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(io::Error::from)?;
    bounded_child_output(&mut child, &mut output, end)
}

fn bounded_child_output(
    child: &mut OracleGuard,
    output: &mut std::process::ChildStdout,
    end: Instant,
) -> io::Result<BoundedOutput> {
    let mut bytes = Vec::new();
    let mut eof = false;
    let mut buffer = [0_u8; 1024];
    loop {
        if Instant::now() >= end {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "owned oracle helper timed out",
            ));
        }
        if !eof {
            match output.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(count) => {
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > 8192 {
                        return Err(io::Error::other("owned oracle helper exceeded output cap"));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let mut descriptors = [PollFd::new(
                        output.as_fd(),
                        PollFlags::POLLIN | PollFlags::POLLHUP,
                    )];
                    let wait = end
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(20) as u16;
                    poll(&mut descriptors, wait).map_err(io::Error::from)?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if eof => return Ok(BoundedOutput { bytes, status }),
            Ok(_) => (),
            Err(error) => return Err(error),
        }
    }
}

pub fn independent_exit_code(expected: i32) -> i32 {
    let code = expected.to_string();
    let result = run_bounded_until(PYTHON, &[FIXTURE, "exit", &code], deadline(3_000)).unwrap();
    assert!(
        result.bytes.is_empty(),
        "exit oracle unexpectedly wrote output"
    );
    result
        .status
        .code()
        .expect("fixture was not observed with an integer exit code")
}

pub fn independent_signal_number() -> i32 {
    let child = Command::new(PYTHON)
        .args([FIXTURE, "hold"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = OracleGuard(child);
    child.kill().unwrap();
    let end = deadline(3_000);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return status
                    .signal()
                    .expect("oracle child was not signal-terminated");
            }
            Ok(None) => (),
            Err(error) => panic!("independent signal oracle wait failed: {error}"),
        }
        if Instant::now() >= end {
            panic!("independent signal oracle wait timed out");
        }
        let wait = end
            .saturating_duration_since(Instant::now())
            .as_millis()
            .clamp(1, 20) as u16;
        let mut no_fds: [PollFd<'_>; 0] = [];
        poll(&mut no_fds, wait).unwrap();
    }
}

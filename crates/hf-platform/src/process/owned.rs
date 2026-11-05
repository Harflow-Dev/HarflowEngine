use super::SpawnSpec;
use super::capture::{self, Capture};
use hf_application::process::IdentityPort;
use hf_domain::process::{Containment, IdentityObservation, ProcessExit, ProcessIdentity};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::fs::File;
use std::io::{self, Write};
use std::time::Instant;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedSignal {
    Terminate,
    Kill,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalOutcome {
    Sent,
    Refused(IdentityObservation),
    DeadlineExpired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitOutcome {
    Exited(ProcessExit),
    DeadlineExpired,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnInitialization {
    Ready,
    UnknownIdentity,
    PipeSetupFailed,
    DeadlineExpired,
}
/// The child is private: this object is the exclusive waiter and signal authority.
/// Unknown identity never discards ownership and never authorizes a signal.
pub struct RetainedOwnedChild {
    child: Pid,
    authority_unknown: bool,
    wait_authority_lost: bool,
    stdin: Option<File>,
    stdout: Option<File>,
    stderr: Option<File>,
    registered: Option<ProcessIdentity>,
    exit: ProcessExit,
    reaped: bool,
    capture: Capture,
    cap: usize,
    pipe_setup_error: Option<String>,
    initialization: SpawnInitialization,
    capture_transferred: bool,
}
impl RetainedOwnedChild {
    /// OS spawn/probe calls are nonpreemptible. A late result retains ownership.
    pub fn spawn(
        spec: SpawnSpec,
        source: &impl IdentityPort,
        deadline: Instant,
    ) -> io::Result<Self> {
        let deadline = deadline.min(Instant::now() + std::time::Duration::from_secs(2));
        check_spawn_deadline(deadline)?;
        spec.validate()?;
        check_spawn_deadline(deadline)?;
        let spawned = super::spawn::spawn(&spec, deadline)?;
        let child = spawned.pid;
        let stdin = Some(spawned.input);
        let stdout = Some(spawned.output);
        let stderr = Some(spawned.error);
        let setup = match (&stdout, &stderr, &stdin) {
            (Some(out), Some(err), Some(input)) => capture::nonblocking(out)
                .and_then(|()| capture::nonblocking(err))
                .and_then(|()| capture::nonblocking(input)),
            _ => Err(io::Error::other("piped stdio absent")),
        };
        let registered = source
            .identity(child.as_raw() as u32)
            .filter(|identity| spawned.qualified && identity.pid() == child.as_raw() as u32);
        let initialization = if Instant::now() >= deadline {
            SpawnInitialization::DeadlineExpired
        } else if setup.is_err() {
            SpawnInitialization::PipeSetupFailed
        } else if registered.is_none() {
            SpawnInitialization::UnknownIdentity
        } else {
            SpawnInitialization::Ready
        };
        Ok(Self {
            child,
            authority_unknown: !spawned.qualified,
            wait_authority_lost: false,
            stdin,
            stdout,
            stderr,
            registered,
            exit: ProcessExit::Unobserved,
            reaped: false,
            capture: Capture::default(),
            cap: spec.capture_limit,
            pipe_setup_error: setup.err().map(|e| e.to_string()),
            initialization,
            capture_transferred: false,
        })
    }
    /// Private fixture transport only; consumes stdout directly without capture
    /// buffering. Do not call `pump` or `wait_until` until the protocol is complete:
    /// those methods also consume stdout. Protocol bytes do not enter capture;
    /// stderr remains separate and must be drained by subsequent capture pumping.
    pub fn read_fixture_frame(&self, deadline: Instant) -> io::Result<Vec<u8>> {
        let stdout = self
            .stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdout absent"))?;
        crate::fixture::read_frame(stdout, deadline)
    }
    pub fn write_fixture_frame(&self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        let input = self
            .stdin
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdin absent"))?;
        crate::fixture::write_all(input, bytes, deadline)
    }
    /// Checks protocol EOF without changing capture EOF or exit evidence.
    pub fn fixture_eof(&self, deadline: Instant) -> io::Result<()> {
        let stdout = self
            .stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdout absent"))?;
        crate::fixture::require_eof(stdout, deadline)
    }
    pub fn initialization(&self) -> SpawnInitialization {
        self.initialization
    }
    pub fn pid(&self) -> u32 {
        self.child.as_raw() as u32
    }
    pub fn registered_identity(&self) -> Option<&ProcessIdentity> {
        self.registered.as_ref()
    }
    pub fn capture(&self) -> &Capture {
        &self.capture
    }
    /// Transfers buffered bytes only; EOF, overflow and error evidence is retained.
    pub fn take_capture_bytes(&mut self) -> (Vec<u8>, Vec<u8>) {
        (
            std::mem::take(&mut self.capture.stdout.bytes),
            std::mem::take(&mut self.capture.stderr.bytes),
        )
    }
    pub fn containment(&self) -> Containment {
        Containment::Unknown
    }
    pub fn exit(&self) -> ProcessExit {
        self.exit
    }
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }
    pub fn pump(&mut self, deadline: Instant, read_budget: usize) -> io::Result<()> {
        if let Some(error) = &self.pipe_setup_error {
            return Err(io::Error::other(error.clone()));
        }
        let (Some(stdout), Some(stderr)) = (&mut self.stdout, &mut self.stderr) else {
            return Err(io::Error::other("capture pipes absent"));
        };
        capture::pump(
            stdout,
            stderr,
            &mut self.capture,
            self.cap,
            read_budget.min(65536),
            deadline,
        )
    }
    /// Private fixed-helper handoff. Failure after FD transfer is classified as
    /// pipe loss; no capture retry or fabricated EOF is permitted.
    pub fn start_private_fixture_capture_reader(
        &mut self,
        origin: super::CaptureOrigin,
        deadline: Instant,
    ) -> io::Result<super::PrivateFixtureCaptureReader> {
        if self.registered.as_ref() != Some(&origin.child)
            || self.initialization != SpawnInitialization::Ready
            || self.capture != Capture::default()
            || self.stdout.is_none()
            || self.stderr.is_none()
            || Instant::now() >= deadline
        {
            return Err(io::Error::other("capture handoff refused before transfer"));
        }
        let out = self
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout absent"))?;
        let err = self
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr absent"))?;
        self.capture_transferred = true;
        super::reader_loop::start(origin, out, err, deadline).map_err(|error| {
            io::Error::other(format!("private capture startup pipe loss: {error}"))
        })
    }
    /// Exit-only observation after independent pipe ownership transfer.
    pub fn wait_exit_until(
        &mut self,
        deadline: Instant,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<WaitOutcome> {
        loop {
            if cancelled() {
                return Ok(WaitOutcome::Cancelled);
            }
            let exit = self.try_wait()?;
            if exit != ProcessExit::Unobserved {
                return Ok(WaitOutcome::Exited(exit));
            }
            if Instant::now() >= deadline {
                return Ok(WaitOutcome::DeadlineExpired);
            }
            std::thread::sleep(
                std::time::Duration::from_millis(1)
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    /// Returns the bytes accepted in this call; callers retain any unsent suffix.
    pub fn write_stdin(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<usize> {
        if Instant::now() >= deadline {
            return Ok(0);
        }
        if let Some(error) = &self.pipe_setup_error {
            return Err(io::Error::other(error.clone()));
        }
        let pipe = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdin closed"))?;
        match pipe.write(&bytes[..bytes.len().min(65536)]) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(0)
            }
            result => result,
        }
    }
    pub fn try_wait(&mut self) -> io::Result<ProcessExit> {
        if self.reaped || self.wait_authority_lost {
            return Ok(self.exit);
        }

        match waitpid(self.child, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                self.reaped = true;
                self.exit = ProcessExit::Code(code);
            }
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                self.reaped = true;
                self.exit = ProcessExit::Signal(signal as i32);
            }
            Ok(_) | Err(nix::errno::Errno::EINTR) => (),
            Err(nix::errno::Errno::ECHILD) => {
                self.authority_unknown = true;
                self.wait_authority_lost = true;
            }
            Err(error) => return Err(error.into()),
        }
        Ok(self.exit)
    }
    pub fn request_signal(
        &mut self,
        expected: &ProcessIdentity,
        source: &impl IdentityPort,
        signal: OwnedSignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        if Instant::now() >= deadline {
            return Ok(SignalOutcome::DeadlineExpired);
        }
        self.try_wait()?;
        if self.authority_unknown {
            return Ok(SignalOutcome::Refused(IdentityObservation::Unknown));
        }
        if self.reaped {
            return Ok(SignalOutcome::Refused(IdentityObservation::Dead));
        }
        if expected.pid() != (self.child.as_raw() as u32)
            || self.registered.as_ref() != Some(expected)
        {
            return Ok(SignalOutcome::Refused(IdentityObservation::Mismatch));
        }
        let observation = source.observe(expected);
        if observation != IdentityObservation::Matching {
            return Ok(SignalOutcome::Refused(observation));
        }
        if Instant::now() >= deadline {
            return Ok(SignalOutcome::DeadlineExpired);
        }
        let pid = i32::try_from(self.child.as_raw() as u32)
            .map_err(|_| io::Error::other("invalid owned PID"))?;
        kill(
            Pid::from_raw(pid),
            match signal {
                OwnedSignal::Terminate => Signal::SIGTERM,
                OwnedSignal::Kill => Signal::SIGKILL,
            },
        )
        .map_err(io::Error::from)?;
        Ok(SignalOutcome::Sent)
    }
    /// Cancellation only stops this observation call; signaling is explicit.
    pub fn wait_until(
        &mut self,
        deadline: Instant,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<WaitOutcome> {
        if self.capture_transferred {
            return self.wait_exit_until(deadline, cancelled);
        }
        loop {
            if cancelled() {
                return Ok(WaitOutcome::Cancelled);
            }
            if Instant::now() >= deadline {
                return Ok(WaitOutcome::DeadlineExpired);
            }
            let exit = self.try_wait()?;
            if exit != ProcessExit::Unobserved {
                return Ok(WaitOutcome::Exited(exit));
            }
            self.pump(deadline, 65536)?;
        }
    }
}

#[cfg(test)]
#[path = "owned/tests.rs"]
mod tests;

fn check_spawn_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "spawn deadline expired before execution",
        ))
    } else {
        Ok(())
    }
}

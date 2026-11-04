use super::SpawnSpec;
use super::capture::{self, Capture};
use hf_application::process::IdentityPort;
use hf_domain::process::{Containment, IdentityObservation, ProcessExit, ProcessIdentity};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io::{self, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
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
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    registered: Option<ProcessIdentity>,
    exit: ProcessExit,
    reaped: bool,
    capture: Capture,
    cap: usize,
    pipe_setup_error: Option<String>,
    initialization: SpawnInitialization,
}
impl RetainedOwnedChild {
    /// OS spawn/probe calls are nonpreemptible. A late result retains ownership.
    pub fn spawn(
        spec: SpawnSpec,
        source: &impl IdentityPort,
        deadline: Instant,
    ) -> io::Result<Self> {
        check_spawn_deadline(deadline)?;
        spec.validate()?;
        check_spawn_deadline(deadline)?;
        let mut child = Command::new(&spec.executable)
            .current_dir(&spec.cwd)
            .args(&spec.args)
            .env_clear()
            .envs(spec.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let setup = match (&stdout, &stderr, &stdin) {
            (Some(out), Some(err), Some(input)) => capture::nonblocking(out)
                .and_then(|()| capture::nonblocking(err))
                .and_then(|()| capture::nonblocking(input)),
            _ => Err(io::Error::other("piped stdio absent")),
        };
        let registered = source
            .identity(child.id())
            .filter(|identity| identity.pid() == child.id());
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
        })
    }
    pub fn initialization(&self) -> SpawnInitialization {
        self.initialization
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn registered_identity(&self) -> Option<&ProcessIdentity> {
        self.registered.as_ref()
    }
    pub fn capture(&self) -> &Capture {
        &self.capture
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
        if self.reaped {
            return Ok(self.exit);
        }
        if let Some(status) = self.child.try_wait()? {
            self.reaped = true;
            self.exit = match (status.code(), status.signal()) {
                (Some(code), _) => ProcessExit::Code(code),
                (_, Some(signal)) => ProcessExit::Signal(signal),
                _ => ProcessExit::Unobserved,
            };
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
        if self.reaped {
            return Ok(SignalOutcome::Refused(IdentityObservation::Dead));
        }
        if expected.pid() != self.child.id() || self.registered.as_ref() != Some(expected) {
            return Ok(SignalOutcome::Refused(IdentityObservation::Mismatch));
        }
        let observation = source.observe(expected);
        if observation != IdentityObservation::Matching {
            return Ok(SignalOutcome::Refused(observation));
        }
        if Instant::now() >= deadline {
            return Ok(SignalOutcome::DeadlineExpired);
        }
        let pid =
            i32::try_from(self.child.id()).map_err(|_| io::Error::other("invalid owned PID"))?;
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

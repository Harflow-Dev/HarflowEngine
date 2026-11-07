//! The calling process's place in the process table, and verdict evidence for recorded pids.
//!
//! The table comes from `/bin/ps`, the setuid helper a user can always run: reading other
//! users' processes through `proc_pidinfo` fails with EPERM on exactly the root-owned
//! ancestors (`login`, `sudo`) an ordinary terminal chain contains. The helper runs with a
//! cleared environment, a deadline and an output cap, and is killed and reaped on overrun.
//! Any malformed line, timeout or nonzero exit is a failure to look.
use hf_application::ctl::task_reads::{ProcessViewPort, ViewFailure};
use hf_domain::{
    ctl::caller::{PidObservation, ProcessView, ancestors_of},
    process::ProcessIdentity,
};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use sysctl::{Ctl, Sysctl};

const PS: &str = "/bin/ps";
/// The most of one command's remaining time the helper may take, and its absolute cap.
const HELPER_BUDGET: Duration = Duration::from_secs(2);
const OUTPUT_CAP: usize = 4 * 1024 * 1024;
/// How long to wait for a killed helper to be reaped before giving up on it.
const REAP: Duration = Duration::from_millis(250);

/// The process view backed by a helper program. Production uses `/bin/ps`; the program path
/// and budget are constructor arguments for tests only and are reachable from no argument
/// or environment variable of the command.
#[derive(Clone, Debug)]
pub struct PsProcessView {
    program: Option<PathBuf>,
    budget: Duration,
}

impl PsProcessView {
    /// The production helper: `/bin/ps` with the standard budget.
    pub const fn system() -> Self {
        Self {
            program: None,
            budget: HELPER_BUDGET,
        }
    }

    /// A helper at `program` that may run for at most `budget` (and half the time left).
    pub fn with_helper(program: impl Into<PathBuf>, budget: Duration) -> Self {
        Self {
            program: Some(program.into()),
            budget,
        }
    }

    fn program(&self) -> &Path {
        self.program.as_deref().unwrap_or(Path::new(PS))
    }

    /// Runs the helper and returns its standard output, or `None` on timeout, overrun or
    /// failure. One absolute deadline covers the read, the wait and the kill:
    /// `min(remaining / 2, budget)`, so the snapshot keeps budget of its own.
    fn run(&self, deadline: Instant) -> Option<Vec<u8>> {
        let now = Instant::now();
        let allowed = (deadline.saturating_duration_since(now) / 2).min(self.budget);
        let helper_deadline = now + allowed;
        let mut child = Command::new(self.program())
            .args(["-axo", "pid=,ppid="])
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let Some(stdout) = child.stdout.take() else {
            terminate(&mut child);
            return None;
        };
        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("ps-reader".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let outcome = stdout.take(OUTPUT_CAP as u64 + 1).read_to_end(&mut bytes);
                let _ = sender.send(outcome.ok().map(|_| bytes));
            });
        let Ok(reader) = spawned else {
            terminate(&mut child);
            return None;
        };
        let output = receiver
            .recv_timeout(helper_deadline.saturating_duration_since(Instant::now()))
            .ok()
            .flatten();
        let finished = output
            .as_ref()
            .and_then(|_| wait_until(&mut child, helper_deadline));
        if finished.is_none() {
            terminate(&mut child);
        }
        // The pipe closes when the helper is gone; a reader still stuck is abandoned.
        if reader.is_finished() || finished.is_some() {
            let _ = reader.join();
        }
        let output = output?;
        (finished? && output.len() <= OUTPUT_CAP).then_some(output)
    }
}

/// Kills the helper and waits a bounded time for it; never blocks without a limit.
fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = wait_until(child, Instant::now() + REAP);
}

/// `Some(true)` when the child exited successfully, `Some(false)` when it failed and
/// `None` on timeout.
fn wait_until(child: &mut std::process::Child, deadline: Instant) -> Option<bool> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
            _ => return None,
        }
    }
}

fn parse_table(bytes: &[u8]) -> Option<Vec<(u32, u32)>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut table = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let pid = fields.next()?.parse().ok()?;
        let parent = fields.next()?.parse().ok()?;
        if fields.next().is_some() {
            return None;
        }
        table.push((pid, parent));
    }
    (!table.is_empty()).then_some(table)
}

fn boot_uuid() -> Option<String> {
    Ctl::new("kern.bootsessionuuid").ok()?.value_string().ok()
}

impl ProcessViewPort for PsProcessView {
    fn view(&self, deadline: Instant) -> Result<ProcessView, ViewFailure> {
        let pid = std::process::id();
        let table = self
            .run(deadline)
            .and_then(|bytes| parse_table(&bytes))
            .ok_or(ViewFailure)?;
        let ancestors = ancestors_of(pid, &table).ok_or(ViewFailure)?;
        let session = nix::unistd::getsid(None).map_err(|_| ViewFailure)?;
        Ok(ProcessView {
            pid,
            session: i64::from(session.as_raw()),
            ancestors,
        })
    }

    /// Presence follows the recovery observer: only a positive ESRCH proves absence, and any
    /// unreadable pid, including another user's, stays unknown.
    fn observe(&self, pid: u32) -> PidObservation {
        let Some(native) = i32::try_from(pid).ok().filter(|pid| *pid > 1) else {
            return PidObservation::Unknown;
        };
        let before = boot_uuid();
        let info = pidinfo::<BSDInfo>(native, 0);
        let absent = info.is_err() && kill(Pid::from_raw(native), None) == Err(Errno::ESRCH);
        let after = boot_uuid();
        let Some(boot) = before.filter(|value| Some(value) == after.as_ref()) else {
            return PidObservation::Unknown;
        };
        match info {
            Ok(info) if info.pbi_pid == pid => match info.pbi_status {
                1..=4 => {
                    ProcessIdentity::new(pid, info.pbi_start_tvsec, info.pbi_start_tvusec, boot)
                        .map_or(PidObservation::Unknown, PidObservation::Present)
                }
                5 => PidObservation::Zombie,
                _ => PidObservation::Unknown,
            },
            Ok(_) => PidObservation::Unknown,
            Err(_) if absent => PidObservation::Absent,
            Err(_) => PidObservation::Unknown,
        }
    }
}

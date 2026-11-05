#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_application::process::IdentityPort;
use hf_domain::process::{ProcessExit, ProcessIdentity};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnInitialization, SpawnSpec,
    WaitOutcome,
};
use std::ffi::OsString;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{Duration, Instant};

const PYTHON: &str = "/usr/bin/python3";
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/process_fixture.py"
);

#[path = "common/oracle.rs"]
mod oracle;
pub(super) use oracle::{
    independent_exit_code, independent_signal_number, run_bounded, run_bounded_until,
};

pub(super) struct FixtureGuard(pub(super) RetainedOwnedChild);
impl Deref for FixtureGuard {
    type Target = RetainedOwnedChild;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for FixtureGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for FixtureGuard {
    fn drop(&mut self) {
        let _ = self.0.write_stdin(b"quit\n", deadline(250));
        self.0.close_stdin();
        let end = deadline(1_000);
        while Instant::now() < end {
            let _ = self.0.pump(end, 16_384);
            let exit = self.0.try_wait();
            if matches!(exit, Ok(ProcessExit::Code(_) | ProcessExit::Signal(_)))
                && self.0.capture().stdout.eof
                && self.0.capture().stderr.eof
            {
                return;
            }
        }
        if let Some(identity) = self.0.registered_identity().cloned()
            && self
                .0
                .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(250))
                .ok()
                == Some(SignalOutcome::Sent)
        {
            let waited = self.0.wait_until(deadline(1_000), || false).ok();
            let _ = self.0.pump(deadline(250), 16_384);
            if matches!(
                waited,
                Some(WaitOutcome::Exited(
                    ProcessExit::Code(_) | ProcessExit::Signal(_)
                ))
            ) && self.0.capture().stdout.eof
                && self.0.capture().stderr.eof
            {
                return;
            }
        }
        eprintln!(
            "fixture cleanup failed for owned pid {}; aborting to avoid a survivor",
            self.0.pid()
        );
        std::process::abort();
    }
}

pub(super) struct FixedIdentity(pub(super) Option<ProcessIdentity>);
impl IdentityPort for FixedIdentity {
    fn identity(&self, _pid: u32) -> Option<ProcessIdentity> {
        self.0.clone()
    }
}

pub(super) struct DelayedIdentity {
    pub(super) observed_pid: SyncSender<u32>,
    pub(super) release: Receiver<()>,
}
impl IdentityPort for DelayedIdentity {
    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        let _ = self.observed_pid.send(pid);
        let _ = self.release.recv_timeout(Duration::from_secs(3));
        MacIdentity.identity(pid)
    }
}

pub(super) fn deadline(milliseconds: u64) -> Instant {
    Instant::now() + Duration::from_millis(milliseconds)
}

pub(super) fn spec(mode: &str, args: &[&str], capture_limit: usize) -> SpawnSpec {
    SpawnSpec {
        executable: PathBuf::from(PYTHON),
        cwd: std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        args: std::iter::once(FIXTURE.to_owned())
            .chain(std::iter::once(mode.to_owned()))
            .chain(args.iter().map(|value| (*value).to_owned()))
            .map(OsString::from)
            .collect(),
        environment: Vec::new(),
        capture_limit,
    }
}

pub(super) fn spawn(mode: &str, args: &[&str], cap: usize) -> FixtureGuard {
    let child =
        RetainedOwnedChild::spawn(spec(mode, args, cap), &MacIdentity, deadline(3_000)).unwrap();
    let fixture = FixtureGuard(child);
    assert_eq!(fixture.initialization(), SpawnInitialization::Ready);
    fixture
}

pub(super) fn pump_until(
    child: &mut RetainedOwnedChild,
    predicate: impl Fn(&RetainedOwnedChild) -> bool,
) {
    let end = deadline(3_000);
    while !predicate(child) && Instant::now() < end {
        child.pump(end, 65_536).unwrap();
    }
    assert!(
        predicate(child),
        "bounded fixture rendezvous did not complete"
    );
}

pub(super) fn write_all(child: &mut RetainedOwnedChild, bytes: &[u8]) {
    let end = deadline(3_000);
    let mut offset = 0;
    while offset < bytes.len() && Instant::now() < end {
        offset += child.write_stdin(&bytes[offset..], end).unwrap();
        child.pump(end, 16_384).unwrap();
    }
    assert_eq!(
        offset,
        bytes.len(),
        "private fixture control write timed out"
    );
}

pub(super) fn cooperative_quit(child: &mut RetainedOwnedChild) {
    write_all(child, b"quit\n");
    child.close_stdin();
    assert!(matches!(
        child.wait_until(deadline(3_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    pump_until(child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
}

pub(super) fn independent_os_probe(pid: u32) -> (u64, u64, u32) {
    let pid_arg = pid.to_string();
    let output = run_bounded(PYTHON, &[FIXTURE, "probe", &pid_arg]).unwrap();
    let values: Vec<u64> = String::from_utf8(output)
        .unwrap()
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    assert_eq!(values.len(), 5);
    assert_eq!(values[0], u64::from(pid));
    assert_eq!(
        values[4] as usize, 136,
        "independent BSDInfo layout size changed"
    );
    (values[1], values[2], values[3] as u32)
}

pub(super) fn independent_boot_uuid() -> String {
    String::from_utf8(run_bounded("/usr/sbin/sysctl", &["-n", "kern.bootsessionuuid"]).unwrap())
        .unwrap()
        .trim()
        .to_ascii_lowercase()
}

pub(super) fn independent_process_state_until(pid: u32, end: Instant) -> String {
    let pid_arg = pid.to_string();
    let result = run_bounded_until("/bin/ps", &["-o", "stat=", "-p", &pid_arg], end).unwrap();
    let state = String::from_utf8(result.bytes).unwrap().trim().to_owned();
    match (result.status.code(), state.is_empty()) {
        (Some(0), _) => state,
        (Some(1), true) => String::new(),
        _ => panic!(
            "ps returned unexpected status {} with state {state:?}",
            result.status
        ),
    }
}

pub(super) fn independent_group(pid: u32) -> u32 {
    let pid_arg = pid.to_string();
    String::from_utf8(run_bounded(PYTHON, &[FIXTURE, "group", &pid_arg]).unwrap())
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

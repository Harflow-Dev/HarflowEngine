#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::process::IdentityPort;
use hf_domain::process::{ProcessExit, ProcessIdentity};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnInitialization, WaitOutcome,
};
use hf_platform::qualification::{FixtureRoot, ProfilePolicy};
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::fd::{AsFd, AsRawFd},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const PYTHON_RUNTIME: &str =
    "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework";
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/qualification/syscall_probe.py"
);

struct SettledRoot {
    fixture: FixtureRoot,
    settled: bool,
}

struct DirectChild {
    child: Option<std::process::Child>,
    registered: ProcessIdentity,
}
impl DirectChild {
    fn new(child: std::process::Child) -> Self {
        let registered = MacIdentity.identity(child.id()).unwrap_or_else(|| {
            eprintln!("direct qualification child identity unavailable; aborting isolated fixture");
            std::process::abort();
        });
        Self {
            child: Some(child),
            registered,
        }
    }

    fn wait_output(&mut self, timeout_ms: u64) -> std::process::Output {
        let child = self.child.as_mut().expect("direct child handle retained");
        let end = deadline(timeout_ms);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < end => std::thread::yield_now(),
                Ok(None) => panic!("direct qualification child did not settle within bound"),
                Err(error) => panic!("direct qualification child wait failed: {error}"),
            }
        };
        let stdout = drain_direct_pipe(child.stdout.as_mut().expect("piped stdout"));
        let stderr = drain_direct_pipe(child.stderr.as_mut().expect("piped stderr"));
        std::process::Output {
            status,
            stdout,
            stderr,
        }
    }
}

impl Drop for DirectChild {
    fn drop(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => (),
            Err(error) => {
                eprintln!("direct child wait authority unavailable: {error}; aborting fixture");
                std::process::abort();
            }
        }
        if MacIdentity.identity(self.registered.pid()).as_ref() != Some(&self.registered) {
            eprintln!("direct child identity changed; aborting isolated fixture");
            std::process::abort();
        }
        if child.kill().is_err() {
            eprintln!("identity-matched direct child kill failed; aborting fixture");
            std::process::abort();
        }
        let end = deadline(2000);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < end => std::thread::yield_now(),
                Ok(None) | Err(_) => {
                    eprintln!("identity-matched direct child did not settle; aborting fixture");
                    std::process::abort();
                }
            }
        }
    }
}

fn drain_direct_pipe(stream: &mut (impl Read + AsRawFd + AsFd)) -> Vec<u8> {
    let flags = OFlag::from_bits_truncate(fcntl(&*stream, FcntlArg::F_GETFL).unwrap());
    fcntl(&*stream, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).unwrap();
    let end = deadline(1000);
    let mut output = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return output,
            Ok(count) => {
                output.extend_from_slice(&buffer[..count]);
                if output.len() > 4096 {
                    eprintln!("direct child output exceeded cap; aborting isolated fixture");
                    std::process::abort();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= end {
                    eprintln!("direct child stream did not reach EOF; aborting isolated fixture");
                    std::process::abort();
                }
                std::thread::yield_now();
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) => {
                eprintln!("direct child stream read failed: {error}; aborting fixture");
                std::process::abort();
            }
        }
    }
}
impl SettledRoot {
    fn create() -> Self {
        Self {
            fixture: FixtureRoot::create().unwrap(),
            settled: false,
        }
    }
    fn settle(&mut self) {
        self.settled = true;
    }
}
impl Drop for SettledRoot {
    fn drop(&mut self) {
        if !self.settled {
            eprintln!(
                "qualification fixture root retained because completion was not recorded: {}",
                self.fixture.root().display()
            );
            return;
        }
        fs::remove_dir_all(self.fixture.root()).expect("remove settled, harness-owned root");
    }
}

fn launch(root: &FixtureRoot, operation: &str, target: &Path) -> (i32, String) {
    let policy = if matches!(operation, "fork" | "posix-spawn") {
        ProfilePolicy::VendorNoFork
    } else {
        ProfilePolicy::ControlledHelper
    };
    launch_with_policy(root, policy, operation, target)
}

fn launch_with_policy(
    root: &FixtureRoot,
    policy: ProfilePolicy,
    operation: &str,
    target: &Path,
) -> (i32, String) {
    let profile = root.profile(policy, &qualification_reads(), None).unwrap();
    profile_compile_control(root, policy, &profile);
    let spec = root
        .spawn_spec(
            &profile,
            Path::new(PYTHON),
            &[
                OsString::from("-I"),
                OsString::from("-S"),
                OsString::from(FIXTURE),
                OsString::from(operation),
                target.as_os_str().to_owned(),
            ],
            Vec::new(),
            4096,
        )
        .unwrap();
    log_profile(operation, policy, &profile);
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline(3000)).unwrap();
    if child.initialization() != SpawnInitialization::Ready {
        settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    let outcome = match child.wait_until(deadline(5000), || false) {
        Ok(outcome) => outcome,
        Err(_) => settle_or_abort(&mut child, WaitOutcome::DeadlineExpired),
    };
    let code = match outcome {
        WaitOutcome::Exited(ProcessExit::Code(code)) => code,
        WaitOutcome::Exited(exit) => fail_settled_child(&mut child, exit),
        other => settle_or_abort(&mut child, other),
    };
    let end = deadline(1000);
    while (!child.capture().stdout.eof || !child.capture().stderr.eof) && Instant::now() < end {
        child.pump(end, 8192).unwrap();
    }
    assert!(
        child.capture().stdout.eof && child.capture().stderr.eof,
        "settled helper streams must reach EOF within bound"
    );
    let (stdout, stderr) = child.take_capture_bytes();
    assert!(
        stderr.is_empty(),
        "helper emitted diagnostic stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let result = String::from_utf8(stdout).unwrap();
    assert!(
        result.starts_with("RESULT "),
        "helper did not report a completed syscall: {result:?}"
    );
    (code, result)
}

fn profile_compile_control(root: &FixtureRoot, policy: ProfilePolicy, profile: &str) {
    let spec = root
        .spawn_spec(profile, Path::new("/usr/bin/true"), &[], Vec::new(), 4096)
        .unwrap();
    log_profile("profile-compile-control", policy, profile);
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline(3000)).unwrap();
    if child.initialization() != SpawnInitialization::Ready {
        settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    match child.wait_until(deadline(5000), || false) {
        Ok(WaitOutcome::Exited(ProcessExit::Code(0))) => {}
        Ok(WaitOutcome::Exited(exit)) => fail_settled_child(&mut child, exit),
        Ok(other) => settle_or_abort(&mut child, other),
        Err(_) => settle_or_abort(&mut child, WaitOutcome::DeadlineExpired),
    }
}

fn settle_or_abort(child: &mut RetainedOwnedChild, outcome: WaitOutcome) -> ! {
    let Some(identity) = child.registered_identity().cloned() else {
        eprintln!("unknown helper identity; aborting fixture");
        std::process::abort()
    };
    if child
        .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(250))
        .ok()
        != Some(SignalOutcome::Sent)
        || !matches!(
            child.wait_until(deadline(1000), || false),
            Ok(WaitOutcome::Exited(_))
        )
    {
        eprintln!("unsettled qualification helper after {outcome:?}; aborting isolated fixture");
        std::process::abort();
    }
    panic!("qualification helper returned unexpected wait outcome after being settled: {outcome:?}")
}

pub(super) fn fail_settled_child(child: &mut RetainedOwnedChild, exit: ProcessExit) -> ! {
    let end = deadline(1000);
    while (!child.capture().stdout.eof || !child.capture().stderr.eof) && Instant::now() < end {
        if child.pump(end, 8192).is_err() {
            break;
        }
    }
    let (stdout, stderr) = child.take_capture_bytes();
    panic!(
        "qualification child settled with {exit:?}; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    )
}

fn deadline(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}

fn qualification_reads() -> [std::path::PathBuf; 5] {
    [
        PYTHON.into(),
        FIXTURE.into(),
        PYTHON_RUNTIME.into(),
        "/usr/bin/true".into(),
        "/bin/sh".into(),
    ]
}

fn log_profile(operation: &str, policy: ProfilePolicy, profile: &str) {
    eprintln!("HARFLOW_OS_PROFILE operation={operation:?} policy={policy:?} profile={profile:?}");
}

#[path = "aliases.rs"]
mod aliases;
#[path = "controls.rs"]
mod controls;
#[path = "path.rs"]
mod path;
#[path = "topology.rs"]
mod topology;
pub(super) use controls::{controlled_route, vendor_no_fork_profile_denies_both_fork_apis};
pub(super) use path::{ancestor_rename_counterexample, path_boundary_and_alias_counterexample};
pub(super) use topology::{
    safe_spawn_closes_unlisted_descriptors, topology_guard_refuses_aliases,
    topology_guard_refuses_replaced_roles,
};

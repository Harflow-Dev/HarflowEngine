#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::common::*;
use hf_application::process::IdentityPort;
use hf_domain::process::{ProcessExit, ProcessIdentity};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnSpec,
};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const PYTHON: &str = "/usr/bin/python3";
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/race_fixture.py"
);

fn race_spec(mode: &str, args: &[&str], cap: usize) -> SpawnSpec {
    SpawnSpec {
        executable: PathBuf::from(PYTHON),
        cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        args: std::iter::once(FIXTURE.to_owned())
            .chain(std::iter::once(mode.to_owned()))
            .chain(args.iter().map(|value| (*value).to_owned()))
            .map(OsString::from)
            .collect(),
        environment: Vec::new(),
        capture_limit: cap,
    }
}

#[test]
fn dual_stream_pressure_preserves_all_raw_bytes_and_partial_utf8() {
    let size = 262_144usize;
    let mut child = FixtureGuard(
        RetainedOwnedChild::spawn(
            race_spec("pressure", &["262144"], 1_048_576),
            &MacIdentity,
            deadline(3_000),
        )
        .unwrap(),
    );
    assert_eq!(
        child.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.len() > size && value.capture().stderr.bytes.len() >= size + 2
    });
    let expected: Vec<u8> = (0..size).map(|index| (index % 256) as u8).collect();
    let mut stdout = expected.clone();
    stdout.push(0xe2);
    let mut stderr: Vec<u8> = expected.iter().map(|byte| 255 - byte).collect();
    stderr.extend_from_slice(b"\x82\xac");
    assert_eq!(child.capture().stdout.bytes, stdout);
    assert_eq!(child.capture().stderr.bytes, stderr);
    assert!(!child.capture().stdout.overflow);
    assert!(!child.capture().stderr.overflow);
    write_all(&mut child, b"quit\n");
    child.close_stdin();
    assert_eq!(
        child.wait_until(deadline(3_000), || false).unwrap(),
        hf_platform::process::WaitOutcome::Exited(ProcessExit::Code(0))
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
    assert_eq!(child.capture().stdout.bytes, stdout);
    assert_eq!(child.capture().stderr.bytes, stderr);
}

struct ExitDuringIdentityCheck {
    expected: ProcessIdentity,
    release: Mutex<File>,
    saw_zombie: AtomicBool,
}

impl IdentityPort for ExitDuringIdentityCheck {
    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        if pid != self.expected.pid() {
            return None;
        }
        let actual = MacIdentity.identity(pid)?;
        if actual != self.expected {
            return Some(actual);
        }
        let mut release = self.release.lock().ok()?;
        release.write_all(b"exit\n").ok()?;
        release.flush().ok()?;
        let end = deadline(3_000);
        loop {
            let state = independent_process_state_until(pid, end);
            if state.starts_with('Z') {
                self.saw_zombie.store(true, Ordering::SeqCst);
                return Some(actual);
            }
            if Instant::now() >= end {
                return None;
            }
        }
    }
}

#[test]
fn exit_between_identity_probe_and_signal_keeps_unreaped_child_pinned() {
    let dir = std::env::temp_dir().join(format!(
        "hf-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let fifo = dir.join("release.fifo");
    let fifo_arg = fifo.to_string_lossy().into_owned();
    let mut child = FixtureGuard(
        RetainedOwnedChild::spawn(
            race_spec("fifo-exit", &[&fifo_arg], 4096),
            &MacIdentity,
            deadline(3_000),
        )
        .unwrap(),
    );
    assert_eq!(
        child.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    let release = OpenOptions::new()
        .write(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(&fifo)
        .unwrap();
    let mut unrelated = spawn("hold", &[], 4096);
    pump_until(&mut unrelated, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    let expected = child.registered_identity().unwrap().clone();
    let port = ExitDuringIdentityCheck {
        expected: expected.clone(),
        release: Mutex::new(release),
        saw_zombie: AtomicBool::new(false),
    };
    let result = child.request_signal(&expected, &port, OwnedSignal::Terminate, deadline(3_000));
    match result {
        Ok(SignalOutcome::Sent) => (),
        Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ESRCH as i32) => (),
        other => panic!(
            "race should reach the signal syscall after observing an unreaped zombie: {other:?}"
        ),
    }
    assert!(
        port.saw_zombie.load(Ordering::SeqCst),
        "identity hook returned before independent zombie observation"
    );
    assert_eq!(child.try_wait().unwrap(), ProcessExit::Code(0));
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
    write_all(&mut unrelated, b"ping survivor\n");
    pump_until(&mut unrelated, |value| {
        value
            .capture()
            .stdout
            .bytes
            .windows(b"PONG survivor\n".len())
            .any(|part| part == b"PONG survivor\n")
    });
    assert_eq!(
        unrelated.try_wait().unwrap(),
        ProcessExit::Unobserved,
        "unrelated owned sentinel exited during the target PID race"
    );
    cooperative_quit(&mut unrelated);
    fs::remove_file(&fifo).unwrap();
    fs::remove_dir(&dir).unwrap();
}

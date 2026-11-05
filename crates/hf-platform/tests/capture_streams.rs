#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_application::process::IdentityPort;
use hf_domain::{ownership::Id, process::ProcessExit};
use hf_platform::{
    fixture::RawCapture,
    process::{
        MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnSpec, WaitOutcome,
    },
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-b2c-stream-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        let path = self.0.join("store");
        fs::create_dir(&path).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct ChildGuard(RetainedOwnedChild);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.close_stdin();
        let until = deadline(2_000);
        match self.0.wait_until(until, || false) {
            Ok(WaitOutcome::Exited(_)) => return,
            Ok(WaitOutcome::DeadlineExpired) => (),
            other => {
                if !std::thread::panicking() {
                    panic!("owned fixture wait uncertain: {other:?}");
                }
                return;
            }
        }
        let Some(identity) = self.0.registered_identity().cloned() else {
            if !std::thread::panicking() {
                panic!("owned fixture identity unavailable");
            }
            return;
        };
        if MacIdentity.identity(identity.pid()).as_ref() != Some(&identity) {
            if !std::thread::panicking() {
                panic!("owned fixture identity no longer matches");
            }
            return;
        }
        if self
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(500))
            .ok()
            != Some(SignalOutcome::Sent)
        {
            if !std::thread::panicking() {
                panic!("qualified fixture signal refused");
            }
            return;
        }
        if !matches!(
            self.0.wait_until(deadline(1_000), || false),
            Ok(WaitOutcome::Exited(_))
        ) && !std::thread::panicking()
        {
            panic!("owned fixture did not exit after qualified signal");
        }
    }
}
fn deadline(milliseconds: u64) -> Instant {
    Instant::now() + Duration::from_millis(milliseconds)
}
fn id() -> Id {
    Id::new("44444444444444444444444444444444".into()).unwrap()
}
fn spawn(script: &Path, cwd: &Path, cap: usize) -> ChildGuard {
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args: vec![
                OsString::from("-I"),
                OsString::from("-S"),
                script.as_os_str().into(),
            ],
            cwd: cwd.to_path_buf(),
            environment: Vec::new(),
            capture_limit: cap,
        },
        &MacIdentity,
        deadline(2_000),
    )
    .unwrap();
    let guard = ChildGuard(child);
    assert!(guard.0.registered_identity().is_some());
    guard
}
fn append_available(
    child: &mut RetainedOwnedChild,
    raw: &mut RawCapture,
    out: &mut u64,
    err: &mut u64,
    until: Instant,
) {
    child.pump(until, 32_768).unwrap();
    let (stdout, stderr) = child.take_capture_bytes();
    if !stdout.is_empty() || !stderr.is_empty() {
        let offsets = raw
            .append(*out, *err, &stdout, &stderr, until, |_| Ok(()))
            .unwrap();
        *out = offsets.stdout;
        *err = offsets.stderr;
    }
}
fn raw_path(root: &Path, operation: &Id, stream: &str) -> PathBuf {
    root.parent()
        .unwrap()
        .join(format!("r03-b2c.{}.{stream}", operation.as_str()))
}

#[test]
fn bounded_fair_pumping_captures_backpressured_binary_streams_and_reaches_eof_before_exit() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let script = scratch.0.join("bounded_writer.py");
    fs::write(
        &script,
        r#"import os, sys
n=65536
out=bytes((i*17+3)%256 for i in range(n))
err=bytes((i*29+7)%256 for i in range(n))
for offset in range(0,n,4096):
 for fd,data in ((1,out),(2,err)):
  view=memoryview(data[offset:offset+4096])
  while view:
   count=os.write(fd,view)
   view=view[count:]
os.close(1)
os.close(2)
sys.stdin.buffer.readline()
"#,
    )
    .unwrap();
    let mut child = spawn(&script, &scratch.0, 16_384);
    let mut raw = RawCapture::create(&root, &operation, deadline(2_000)).unwrap();
    let mut stdout = 0;
    let mut stderr = 0;
    let until = deadline(5_000);
    while !(child.0.capture().stdout.eof && child.0.capture().stderr.eof) {
        assert!(Instant::now() < until, "bounded stream drain timed out");
        append_available(&mut child.0, &mut raw, &mut stdout, &mut stderr, until);
    }
    assert_eq!((stdout, stderr), (65_536, 65_536));
    assert_eq!(
        child.0.try_wait().unwrap(),
        ProcessExit::Unobserved,
        "both pipe EOFs arrived while the child remained alive"
    );
    child.0.write_stdin(b"release\n", until).unwrap();
    child.0.close_stdin();
    assert!(matches!(
        child.0.wait_until(until, || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    assert_eq!(
        fs::read(raw_path(&root, &operation, "stdout")).unwrap(),
        (0..65_536)
            .map(|i| ((i * 17 + 3) % 256) as u8)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fs::read(raw_path(&root, &operation, "stderr")).unwrap(),
        (0..65_536)
            .map(|i| ((i * 29 + 7) % 256) as u8)
            .collect::<Vec<_>>()
    );
    assert!(!child.0.capture().stdout.overflow && !child.0.capture().stderr.overflow);
    assert!(child.0.capture().stdout.error.is_none() && child.0.capture().stderr.error.is_none());
}

#[test]
fn observed_child_exit_can_precede_capture_eof_until_buffered_output_is_drained() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let script = scratch.0.join("exit_writer.py");
    fs::write(
        &script,
        "import os\nos.write(1,b'out\\x00')\nos.write(2,b'err\\xff')\n",
    )
    .unwrap();
    let mut child = spawn(&script, &scratch.0, 1024);
    let until = deadline(3_000);
    loop {
        let exit = child.0.try_wait().unwrap();
        if exit != ProcessExit::Unobserved {
            break;
        }
        assert!(Instant::now() < until, "bounded exit observation timed out");
        std::thread::yield_now();
    }
    assert!(!child.0.capture().stdout.eof && !child.0.capture().stderr.eof);
    assert_eq!(child.0.capture().stdout.bytes, b"");
    let mut raw = RawCapture::create(&root, &operation, deadline(1_000)).unwrap();
    let mut stdout = 0;
    let mut stderr = 0;
    while !(child.0.capture().stdout.eof && child.0.capture().stderr.eof) {
        assert!(Instant::now() < until, "bounded post-exit drain timed out");
        append_available(&mut child.0, &mut raw, &mut stdout, &mut stderr, until);
    }
    assert_eq!(
        fs::read(raw_path(&root, &operation, "stdout")).unwrap(),
        b"out\x00"
    );
    assert_eq!(
        fs::read(raw_path(&root, &operation, "stderr")).unwrap(),
        b"err\xff"
    );
    assert!(matches!(child.0.exit(), ProcessExit::Code(0)));
}

#[test]
fn take_bytes_does_not_clear_overflow_or_eof_observation() {
    let scratch = Scratch::new();
    let script = scratch.0.join("overflow_writer.py");
    fs::write(
        &script,
        "import os\nos.write(1,b'out-too-long')\nos.write(2,b'err-too-long')\n",
    )
    .unwrap();
    let mut child = spawn(&script, &scratch.0, 4);
    let until = deadline(2_000);
    assert!(matches!(
        child.0.wait_until(until, || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    while !(child.0.capture().stdout.eof && child.0.capture().stderr.eof) {
        assert!(
            Instant::now() < until,
            "bounded post-exit EOF observation timed out"
        );
        child.0.pump(until, 16_384).unwrap();
    }
    assert!(child.0.capture().stdout.eof && child.0.capture().stderr.eof);
    assert!(child.0.capture().stdout.overflow && child.0.capture().stderr.overflow);
    let (stdout, stderr) = child.0.take_capture_bytes();
    assert_eq!(stdout.len(), 4);
    assert_eq!(stderr.len(), 4);
    assert!(child.0.take_capture_bytes() == (Vec::new(), Vec::new()));
    assert!(child.0.capture().stdout.eof && child.0.capture().stderr.eof);
    assert!(child.0.capture().stdout.overflow && child.0.capture().stderr.overflow);
}

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use hf_domain::{
    fixture::{FixtureFault, Frame, Gate, ParentPlan, QualifiedLedger},
    ownership::{Digest, Id, PreparedRequest},
};
use hf_interfaces::fixture::{decode, encode};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome,
};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[path = "../common/fixture_guard.rs"]
mod fixture_guard;

pub(super) const PYTHON: &str = "/usr/bin/python3";
pub(super) const OP: &str = "11111111111111111111111111111111";
pub(super) const SESSION: &str = "22222222222222222222222222222222";
pub(super) const RESERVATION: &str = "33333333333333333333333333333333";
pub(super) const ATTEMPT: &str = "44444444444444444444444444444444";
pub(super) const GENERATION: &str = "55555555555555555555555555555555";
pub(super) const OWNER: &str = "66666666666666666666666666666666";
pub(super) const NONCE: &str = "77777777777777777777777777777777";
pub(super) const EMPTY_DIGEST: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(super) struct Scratch(
    pub(super) PathBuf,
    #[allow(dead_code)] fixture_guard::FixtureGuard,
);
impl Scratch {
    pub(super) fn new() -> Self {
        let guard = fixture_guard::acquire();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            PathBuf::from("/private/tmp").join(format!("hf-b2-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(
            fs::canonicalize(path).expect("canonical isolated test directory"),
            guard,
        )
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct ChildGuard(pub(super) RetainedOwnedChild);
impl ChildGuard {
    pub(super) fn spawn(executable: PathBuf, args: Vec<OsString>, cwd: PathBuf) -> Self {
        let child = RetainedOwnedChild::spawn(
            SpawnSpec {
                executable,
                args,
                cwd,
                environment: Vec::new(),
                capture_limit: 8192,
            },
            &MacIdentity,
            deadline(10),
        )
        .expect("spawn qualified fixture");
        let guard = Self(child);
        assert_eq!(guard.0.initialization(), SpawnInitialization::Ready);
        guard
    }
    pub(super) fn finish(&mut self) {
        let outcome = self
            .0
            .wait_until(deadline(10), || false)
            .expect("bounded child wait");
        assert!(
            matches!(
                outcome,
                WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(0))
            ),
            "fixture did not exit successfully: {outcome:?}"
        );
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.close_stdin();
        let cleanup = match self.0.wait_until(deadline(2), || false) {
            Ok(WaitOutcome::Exited(_)) => return,
            Ok(WaitOutcome::DeadlineExpired) => self.cleanup_owned(),
            Ok(WaitOutcome::Cancelled) => Err("owned child wait cancelled"),
            Err(_) => Err("owned child wait failed"),
        };
        if let Err(reason) = cleanup {
            if std::thread::panicking() {
                eprintln!("fixture child cleanup failed: {reason}");
            } else {
                panic!("fixture child cleanup failed: {reason}");
            }
        }
    }
}
impl ChildGuard {
    fn cleanup_owned(&mut self) -> Result<(), &'static str> {
        let identity = self
            .0
            .registered_identity()
            .cloned()
            .ok_or("unknown child identity")?;
        match self
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(1))
        {
            Ok(hf_platform::process::SignalOutcome::Sent) => {}
            _ => return Err("fresh qualified signal refused"),
        }
        match self.0.wait_until(deadline(2), || false) {
            Ok(WaitOutcome::Exited(_)) => Ok(()),
            _ => Err("signaled child was not reaped within bound"),
        }
    }
}

pub(super) fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
pub(super) fn id(value: &str) -> Id {
    Id::new(value.to_owned()).expect("fixed ID")
}
pub(super) fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id(OP),
        session: id(SESSION),
        reservation: id(RESERVATION),
        attempt: id(ATTEMPT),
        generation: id(GENERATION),
        digest: Digest::new(EMPTY_DIGEST.into()).expect("empty digest"),
        payload: Vec::new(),
    }
}
pub(super) fn ledger(path: &Path) -> QualifiedLedger {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("exclusive test ledger");
    let metadata = file.metadata().expect("ledger metadata");
    let identity = QualifiedLedger {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    drop(file);
    identity
}
pub(super) fn parent_plan(
    root: &Path,
    ledger: QualifiedLedger,
    gate: Option<Gate>,
    fault: FixtureFault,
) -> Vec<u8> {
    encode(&Frame::Parent(ParentPlan {
        root: root.to_string_lossy().into_owned(),
        request: request(),
        initialize: true,
        launch: true,
        owner: id(OWNER),
        nonce: id(NONCE),
        gate,
        ledger,
        fault,
    }))
    .expect("encode parent plan")
}
pub(super) fn launch_parent(scratch: &Scratch, input: &[u8]) -> ChildGuard {
    let mut child = ChildGuard::spawn(
        PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold")),
        vec!["--internal-parent-fixture-v1".into()],
        scratch.0.clone(),
    );
    child
        .0
        .write_fixture_frame(input, deadline(2))
        .expect("send parent plan");
    child.0.close_stdin();
    child
}
pub(super) fn response(child: &RetainedOwnedChild) -> Frame {
    decode(
        &child
            .read_fixture_frame(deadline(10))
            .expect("bounded private response"),
    )
    .expect("valid response frame")
}
pub(super) fn finish_protocol(child: &mut ChildGuard) {
    child.0.fixture_eof(deadline(10)).expect("response EOF");
    child.finish();
}
mod oracle;
pub(super) use oracle::*;

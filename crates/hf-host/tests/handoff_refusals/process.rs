#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use hf_domain::{
    fixture::{FixtureFault, Frame, ParentPlan, QualifiedLedger, ReportKind},
    ownership::{Digest, Id, PreparedRequest},
    process::ProcessExit,
};
use hf_interfaces::fixture::{decode, encode};
use hf_platform::process::{
    MacIdentity, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome,
};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) const OP: &str = "11111111111111111111111111111111";
const SESSION: &str = "22222222222222222222222222222222";
const RESERVATION: &str = "33333333333333333333333333333333";
const ATTEMPT: &str = "44444444444444444444444444444444";
const GENERATION: &str = "55555555555555555555555555555555";
pub(super) const OWNER: &str = "66666666666666666666666666666666";
pub(super) const NONCE: &str = "77777777777777777777777777777777";
const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(crate) struct Scratch(pub(crate) PathBuf);
impl Scratch {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from("/private/tmp")
            .join(format!("hf-b2-refusal-{}-{seq}", std::process::id()));
        fs::create_dir(&path).expect("create isolated fixture directory");
        Self(fs::canonicalize(path).expect("canonical fixture directory"))
    }
    fn root(&self) -> PathBuf {
        self.0.join("store")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct ChildGuard(pub(crate) RetainedOwnedChild);
impl ChildGuard {
    pub(super) fn spawn(
        executable: PathBuf,
        args: Vec<OsString>,
        _cwd: PathBuf,
        until: Instant,
    ) -> Self {
        Self::spawn_with_env(executable, args, _cwd, Vec::new(), until)
    }
    pub(super) fn spawn_with_env(
        executable: PathBuf,
        args: Vec<OsString>,
        _cwd: PathBuf,
        environment: Vec<(OsString, OsString)>,
        until: Instant,
    ) -> Self {
        let child = RetainedOwnedChild::spawn(
            SpawnSpec {
                executable,
                args,
                cwd: std::fs::canonicalize(std::env::current_dir().unwrap())
                    .expect("canonical inherited working directory"),
                environment,
                capture_limit: 8192,
            },
            &MacIdentity,
            until,
        )
        .expect("spawn retained fixture process");
        let guard = Self(child);
        assert_eq!(guard.0.initialization(), SpawnInitialization::Ready);
        guard
    }
    pub(super) fn finish(&mut self, until: Instant, code: i32) {
        let outcome = self
            .0
            .wait_until(until, || false)
            .expect("bounded owned child wait");
        assert!(
            matches!(outcome, WaitOutcome::Exited(ProcessExit::Code(found)) if found == code),
            "unexpected process outcome: {outcome:?}"
        );
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        super::cleanup::cleanup(&mut self.0);
    }
}

pub(crate) struct Seed {
    pub(crate) scratch: Scratch,
    pub(crate) root: PathBuf,
    pub(crate) ledger: QualifiedLedger,
    pub(crate) request: PreparedRequest,
    pub(crate) generation: Id,
    pub(crate) snapshot: String,
    pub(crate) fault: FixtureFault,
}
impl Seed {
    pub(crate) fn new() -> Self {
        let scratch = Scratch::new();
        let root = scratch.root();
        let ledger = create_ledger(&scratch.0.join("r03-b2.effects"));
        let initial = request();
        let plan = Frame::Parent(ParentPlan {
            root: root.to_string_lossy().into_owned(),
            request: initial.clone(),
            initialize: true,
            launch: false,
            owner: id(OWNER),
            nonce: id(NONCE),
            gate: None,
            ledger,
            fault: FixtureFault::None,
        });
        let bytes = encode(&plan).expect("encode prepare-only parent frame");
        let mut parent = launch_parent(&scratch, &bytes);
        let Frame::Report {
            generation,
            kind: ReportKind::Prepared,
            ..
        } = response(&parent.0)
        else {
            panic!("fixture root was not prepared");
        };
        parent
            .0
            .fixture_eof(deadline(10))
            .expect("prepare report EOF");
        parent.finish(deadline(10), 0);
        let request = PreparedRequest {
            generation: generation.clone(),
            ..initial
        };
        let snapshot = run_oracle(&root, OP, &scratch.0);
        Self {
            scratch,
            root,
            ledger,
            request,
            generation,
            snapshot,
            fault: FixtureFault::None,
        }
    }
}

pub(super) fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
pub(super) fn id(value: &str) -> Id {
    Id::new(value.to_owned()).expect("fixed identifier")
}
pub(super) fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id(OP),
        session: id(SESSION),
        reservation: id(RESERVATION),
        attempt: id(ATTEMPT),
        generation: id(GENERATION),
        digest: Digest::new(EMPTY_DIGEST.into()).expect("digest"),
        payload: Vec::new(),
    }
}
pub(super) fn create_ledger(path: &Path) -> QualifiedLedger {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("exclusive ledger");
    let metadata = file.metadata().expect("ledger metadata");
    QualifiedLedger {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
pub(super) fn launch_parent(scratch: &Scratch, bytes: &[u8]) -> ChildGuard {
    let mut child = ChildGuard::spawn(
        PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold")),
        vec![OsString::from("--internal-parent-fixture-v1")],
        scratch.0.clone(),
        deadline(10),
    );
    child
        .0
        .write_fixture_frame(bytes, deadline(2))
        .expect("send prepare-only plan");
    child.0.close_stdin();
    child
}
pub(super) fn response(child: &RetainedOwnedChild) -> Frame {
    decode(
        &child
            .read_fixture_frame(deadline(10))
            .expect("bounded response frame"),
    )
    .expect("decode response frame")
}
pub(super) fn create_owner(until: Instant, cwd: &Path) -> ChildGuard {
    ChildGuard::spawn(
        PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold")),
        vec![OsString::from("--internal-owner-fixture-v1")],
        cwd.to_path_buf(),
        until,
    )
}
pub(super) fn run_oracle(root: &Path, operation: &str, cwd: &Path) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/readonly_oracle.py");
    let until = deadline(5);
    let mut oracle = ChildGuard::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            OsString::from("-I"),
            OsString::from("-S"),
            script.into_os_string(),
            root.as_os_str().to_owned(),
            operation.into(),
        ],
        cwd.to_path_buf(),
        until,
    );
    oracle.0.close_stdin();
    oracle.finish(until, 0);
    drain_capture(&mut oracle, until);
    String::from_utf8(oracle.0.capture().stdout.bytes.clone()).expect("oracle UTF-8")
}

fn drain_capture(child: &mut ChildGuard, until: Instant) {
    loop {
        let capture = child.0.capture();
        assert!(
            capture.stdout.error.is_none(),
            "oracle stdout capture error: {:?}",
            capture.stdout.error
        );
        assert!(
            capture.stderr.error.is_none(),
            "oracle stderr capture error: {:?}",
            capture.stderr.error
        );
        assert!(
            !capture.stdout.overflow && !capture.stderr.overflow,
            "oracle output exceeded capture bound"
        );
        if capture.stdout.eof && capture.stderr.eof {
            return;
        }
        assert!(
            Instant::now() < until,
            "oracle output streams did not reach EOF before deadline"
        );
        child
            .0
            .pump(until, 65536)
            .expect("bounded oracle output drain");
    }
}

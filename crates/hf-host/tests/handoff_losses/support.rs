use hf_domain::{
    fixture::{FixtureFault, Frame, Invitation, ParentPlan, Prepared, QualifiedLedger},
    ownership::{Digest, Id, PreparedRequest},
};
use hf_interfaces::fixture::{decode, encode};
use hf_platform::process::{MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnSpec, WaitOutcome};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    os::unix::{fs::MetadataExt, net::UnixListener},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[path = "../common/fixture_guard.rs"]
mod fixture_guard;

pub const OP: &str = "11111111111111111111111111111111";
pub const SESSION: &str = "22222222222222222222222222222222";
pub const NONCE: &str = "77777777777777777777777777777777";
const PYTHON: &str = "/usr/bin/python3";

pub struct Fixture {
    pub scratch: PathBuf,
    pub root: PathBuf,
    pub effects: PathBuf,
    ledger: QualifiedLedger,
    _fixture_guard: fixture_guard::FixtureGuard,
}
impl Fixture {
    pub fn new() -> Self {
        let guard = fixture_guard::acquire();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            PathBuf::from("/private/tmp").join(format!("hf-b2loss-{}-{n}", std::process::id()));
        fs::create_dir(&path).expect("create isolated scratch");
        let scratch = fs::canonicalize(path).expect("canonical scratch");
        let effects = scratch.join("r03-b2.effects");
        let ledger = create_ledger(&effects);
        Self {
            root: scratch.join("store"),
            scratch,
            effects,
            ledger,
            _fixture_guard: guard,
        }
    }
    pub fn bind(&self, n: u8) -> (UnixListener, PathBuf) {
        let path = self.scratch.join(format!("gate-{n}.sock"));
        let listener = UnixListener::bind(&path).expect("bind private gate");
        listener.set_nonblocking(true).expect("nonblocking gate");
        (listener, path)
    }
    pub fn plan(&self) -> ParentPlan {
        self.plan_at(&self.root, self.ledger)
    }
    pub fn plan_at(&self, root: &Path, ledger: QualifiedLedger) -> ParentPlan {
        ParentPlan {
            root: root.to_string_lossy().into_owned(),
            request: request_for(OP, SESSION, &fixed_generation()),
            initialize: true,
            launch: true,
            owner: id("66666666666666666666666666666666"),
            nonce: id(NONCE),
            gate: None,
            ledger,
            fault: FixtureFault::None,
        }
    }
    pub fn plan_for(&self, request: PreparedRequest) -> ParentPlan {
        let mut p = self.plan();
        p.request = request;
        p
    }
    pub fn parent(&self, plan: ParentPlan) -> Owned {
        let mut child = Owned::spawn(
            binary(),
            vec!["--internal-parent-fixture-v1".into()],
            self.scratch.clone(),
        );
        child
            .0
            .write_fixture_frame(
                &encode(&Frame::Parent(plan)).expect("encode parent plan"),
                deadline(2),
            )
            .expect("write plan");
        child.0.close_stdin();
        child
    }
    pub fn prepare_only(&self, plan: ParentPlan) -> (Frame, Id) {
        let mut plan = plan;
        plan.launch = false;
        let mut child = self.parent(plan);
        let frame = response(&child.0);
        let generation = match &frame {
            Frame::Report { generation, .. } => generation.clone(),
            _ => panic!("expected report: {frame:?}"),
        };
        finish(&mut child);
        (frame, generation)
    }
    pub fn direct_owner(&self, mut plan: ParentPlan) -> Owned {
        plan.initialize = false;
        let mut owner = Owned::spawn(
            binary(),
            vec!["--internal-owner-fixture-v1".into()],
            self.scratch.clone(),
        );
        let Frame::Hello(process) = response(&owner.0) else {
            panic!("owner must identify itself")
        };
        let invitation = Invitation {
            operation: plan.request.operation.clone(),
            generation: plan.request.generation.clone(),
            digest: plan.request.digest.clone(),
            owner: plan.owner.clone(),
            nonce: plan.nonce.clone(),
            process,
            revision: 0,
        };
        send(&owner.0, &Frame::Invitation(invitation.clone()));
        assert_eq!(response(&owner.0), Frame::Ready(invitation.clone()));
        send(
            &owner.0,
            &Frame::Prepared(Prepared {
                invitation,
                request: plan.request.clone(),
                root: plan.root.clone(),
                gate: plan.gate.clone(),
                ledger: plan.ledger,
                fault: plan.fault,
            }),
        );
        owner.0.close_stdin();
        owner
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

pub struct Owned(pub RetainedOwnedChild);
impl Owned {
    pub fn spawn(executable: PathBuf, args: Vec<OsString>, cwd: PathBuf) -> Self {
        let child = RetainedOwnedChild::spawn(
            SpawnSpec {
                executable,
                args,
                cwd,
                environment: Vec::new(),
                capture_limit: 1_048_576,
            },
            &MacIdentity,
            deadline(9),
        )
        .expect("spawn owned fixture");
        let owned = Self(child);
        assert_eq!(
            owned.0.initialization(),
            hf_platform::process::SpawnInitialization::Ready
        );
        owned
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.close_stdin();
        match self.0.wait_until(deadline(1), || false) {
            Ok(WaitOutcome::Exited(_)) => return,
            Ok(WaitOutcome::DeadlineExpired) => {}
            outcome => {
                if !std::thread::panicking() {
                    panic!("direct fixture child wait is unknown: {outcome:?}");
                }
                return;
            }
        }
        let Some(identity) = self.0.registered_identity().cloned() else {
            if !std::thread::panicking() {
                panic!("direct fixture identity is unknown; refusing to signal");
            }
            return;
        };
        if !matches!(
            self.0
                .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(1)),
            Ok(hf_platform::process::SignalOutcome::Sent)
        ) {
            if !std::thread::panicking() {
                panic!("fresh direct-child signal qualification refused");
            }
            return;
        }
        if !matches!(
            self.0.wait_until(deadline(2), || false),
            Ok(WaitOutcome::Exited(_))
        ) && !std::thread::panicking()
        {
            panic!("direct fixture child did not reap after qualified signal");
        }
    }
}

pub fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold"))
}
pub fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
pub fn id(value: &str) -> Id {
    Id::new(value.into()).expect("fixed ID")
}
pub fn fixed_generation() -> Id {
    id("55555555555555555555555555555555")
}
pub fn request_for(operation: &str, session: &str, generation: &Id) -> PreparedRequest {
    let reservation = format!("3333{}", &operation[4..]);
    let attempt = format!("4444{}", &operation[4..]);
    PreparedRequest {
        operation: id(operation),
        session: id(session),
        reservation: id(&reservation),
        attempt: id(&attempt),
        generation: generation.clone(),
        digest: Digest::new(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        )
        .expect("digest"),
        payload: Vec::new(),
    }
}
pub fn create_ledger(path: &Path) -> QualifiedLedger {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("exclusive ledger");
    let metadata = file.metadata().expect("ledger metadata");
    let result = QualifiedLedger {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    drop(file);
    result
}
pub fn response(child: &RetainedOwnedChild) -> Frame {
    decode(
        &child
            .read_fixture_frame(deadline(10))
            .expect("read fixture response"),
    )
    .expect("decode fixture response")
}
fn send(child: &RetainedOwnedChild, frame: &Frame) {
    child
        .write_fixture_frame(&encode(frame).expect("encode owner handoff"), deadline(2))
        .expect("write owner handoff");
}
pub fn finish(child: &mut Owned) {
    child.0.fixture_eof(deadline(2)).expect("protocol EOF");
    assert!(
        matches!(
            child.0.wait_until(deadline(2), || false),
            Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
                0
            )))
        ),
        "fixture exited unsuccessfully"
    );
}
pub fn finish_protocol(child: &mut Owned) {
    finish(child);
}
pub fn kill_direct(child: &mut Owned) {
    let identity = child
        .0
        .registered_identity()
        .expect("owned direct identity")
        .clone();
    assert!(matches!(
        child
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(1)),
        Ok(hf_platform::process::SignalOutcome::Sent)
    ));
    assert!(matches!(
        child.0.wait_until(deadline(2), || false),
        Ok(WaitOutcome::Exited(
            hf_domain::process::ProcessExit::Signal(9)
        ))
    ));
}
#[path = "support/observe.rs"]
mod observe;
pub use observe::{assert_empty, echo_gate, gate_peer, number, oracle};
#[path = "support/process.rs"]
mod process;
pub use process::{
    drain_capture, effect_child_pid, identity_is_current, oracle_generation, oracle_process,
    process_child_pid, run_python_fixture, wait_identity_gone,
};

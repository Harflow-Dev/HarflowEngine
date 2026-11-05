#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::{owner_runtime::DigestPort, process::IdentityPort};
use hf_domain::{
    fixture::{FixtureFault, Gate, Invitation, Prepared, QualifiedLedger, Stage},
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    ownership::{Digest, Id, PreparedRequest},
    process::ProcessIdentity,
};
use hf_interfaces::owner_preparation;
use hf_platform::{
    fixture::{gate_packet, read_frame, write_all},
    process::MacIdentity,
};
use hf_storage::{Limits, Sha256Digest, Store};
use std::{
    fs::{self, OpenOptions},
    os::unix::{fs::MetadataExt, net::UnixListener},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) const OP: &str = "11111111111111111111111111111111";
pub(super) const SESSION: &str = "22222222222222222222222222222222";
pub(super) const RESERVATION: &str = "33333333333333333333333333333333";
pub(super) const ATTEMPT: &str = "44444444444444444444444444444444";
pub(super) const OWNER: &str = "66666666666666666666666666666666";
pub(super) const NONCE: &str = "77777777777777777777777777777777";

pub(super) fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
pub(super) fn id(value: &str) -> Id {
    Id::new(value.to_owned()).unwrap()
}
pub(super) fn digest(value: &str) -> Digest {
    Digest::new(value.repeat(64)).unwrap()
}
pub(super) fn self_identity() -> ProcessIdentity {
    MacIdentity
        .identity(std::process::id())
        .expect("current process identity")
}
pub(super) fn built_scaffold() -> PathBuf {
    let test = std::env::current_exe().expect("test executable path");
    test.parent()
        .and_then(|deps| deps.parent())
        .expect("target debug directory")
        .join("harflow-engine-scaffold")
}

pub(super) struct Scratch {
    pub(super) path: PathBuf,
    pub(super) root: PathBuf,
    pub(super) ledger: QualifiedLedger,
    pub(super) request: PreparedRequest,
}
impl Scratch {
    pub(super) fn new(caller: ProcessIdentity) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from("/private/tmp")
            .join(format!("hf-owner-runtime-{}-{n}", std::process::id()));
        fs::create_dir(&path).expect("create isolated owner fixture");
        let path = fs::canonicalize(path).expect("canonical owner fixture");
        let root = path.join("store");
        let ledger_path = path.join("r03-b2.effects");
        let ledger_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ledger_path)
            .expect("create fixed effect ledger");
        let metadata = ledger_file.metadata().expect("ledger metadata");
        let ledger = QualifiedLedger {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        drop(ledger_file);

        let store = Store::initialize_fixture(
            &root,
            Limits::default(),
            deadline(3),
            hf_platform::lease::shared(),
        )
        .expect("initialize isolated store");
        let generation = id(&store.identity().generation);
        let operation = id(OP);
        let attempt = id(ATTEMPT);
        let envelope = PreparationEnvelope {
            profile_digest: Digest::new(owner_preparation::PROFILE_DIGEST.into()).unwrap(),
            configuration_digest: digest("a"),
            operation: operation.clone(),
            session: id(SESSION),
            reservation: id(RESERVATION),
            attempt: attempt.clone(),
            generation: generation.clone(),
            policy: RuntimePolicy {
                background_requirement: Requirement::Required,
                permission_mode: PermissionMode::OsEnforced,
                background_idle_seconds: 600.0,
                tool_level_opt_in: false,
            },
            authority: AuthorityContext {
                caller,
                role: AuthorityRole::Direct,
                checkpoint: None,
                revision: None,
                issued_attempt: attempt,
                grant_digest: digest("b"),
            },
            assignment: FixtureAssignment::FixedEffect,
        };
        let payload = owner_preparation::encode(&envelope).expect("encode static preparation");
        let request = PreparedRequest {
            operation,
            session: envelope.session,
            reservation: envelope.reservation,
            attempt: envelope.attempt,
            generation,
            digest: Sha256Digest.digest_parts(&[&payload]),
            payload,
        };
        store
            .ownership_prepare(&request, deadline(3))
            .expect("prepare immutable fixture request");
        drop(store);
        Self {
            path,
            root,
            ledger,
            request,
        }
    }

    pub(super) fn prepared(&self, process: ProcessIdentity, gate: Gate) -> Prepared {
        let invitation = Invitation {
            operation: self.request.operation.clone(),
            generation: self.request.generation.clone(),
            digest: self.request.digest.clone(),
            owner: id(OWNER),
            nonce: id(NONCE),
            process,
            revision: 0,
        };
        Prepared {
            invitation,
            request: self.request.clone(),
            root: self.root.to_string_lossy().into_owned(),
            gate: Some(gate),
            ledger: self.ledger,
            fault: FixtureFault::None,
        }
    }

    pub(super) fn gate_listener(&self) -> (UnixListener, PathBuf) {
        let path = self.path.join("writer-gate.sock");
        let listener = UnixListener::bind(&path).expect("bind writer gate");
        listener
            .set_nonblocking(true)
            .expect("set gate listener nonblocking");
        (listener, path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) struct GatePeer {
    arrived: std::sync::Arc<AtomicBool>,
    release: Option<SyncSender<()>>,
    join: Option<JoinHandle<Result<(), String>>>,
}
impl GatePeer {
    pub(super) fn start(listener: UnixListener, stage: Stage, operation: Id, nonce: Id) -> Self {
        let arrived = std::sync::Arc::new(AtomicBool::new(false));
        let observed = arrived.clone();
        let (release, receiver) = mpsc::sync_channel(1);
        let join = thread::spawn(move || {
            gate_thread(listener, stage, operation, nonce, observed, receiver)
        });
        Self {
            arrived,
            release: Some(release),
            join: Some(join),
        }
    }
    pub(super) fn arrived(&self) -> bool {
        self.arrived.load(Ordering::Acquire)
    }
    pub(super) fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        self.release();
        join_bounded(&mut self.join, deadline(3))
    }
}
impl Drop for GatePeer {
    fn drop(&mut self) {
        self.release();
        if join_bounded(&mut self.join, deadline(3)).is_err() {
            std::process::abort();
        }
    }
}

fn gate_thread(
    listener: UnixListener,
    stage: Stage,
    operation: Id,
    nonce: Id,
    arrived: std::sync::Arc<AtomicBool>,
    release: Receiver<()>,
) -> Result<(), String> {
    let until = deadline(9);
    let (stream, _) = loop {
        match release.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                return Err("gate released before writer arrived".into());
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
        match listener.accept() {
            Ok(value) => break value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(error) => return Err(format!("gate accept: {error}")),
        }
        if Instant::now() >= until {
            return Err("gate accept deadline".into());
        }
        thread::yield_now();
    };
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    let packet = read_frame(&stream, until).map_err(|e| e.to_string())?;
    if packet != gate_packet(stage, &operation, &nonce) {
        return Err("gate packet mismatch".into());
    }
    arrived.store(true, Ordering::Release);
    release
        .recv_timeout(until.saturating_duration_since(Instant::now()))
        .map_err(|e| format!("gate release: {e}"))?;
    write_all(&stream, &packet, until).map_err(|e| e.to_string())?;
    drop(stream);
    Ok(())
}

fn join_bounded(
    join: &mut Option<JoinHandle<Result<(), String>>>,
    until: Instant,
) -> Result<(), String> {
    let Some(handle) = join.as_ref() else {
        return Ok(());
    };
    while !handle.is_finished() {
        if Instant::now() >= until {
            return Err("gate thread did not finish within bound".into());
        }
        thread::yield_now();
    }
    join.take()
        .ok_or("gate thread handle absent")?
        .join()
        .map_err(|_| "gate thread panicked".to_owned())?
}

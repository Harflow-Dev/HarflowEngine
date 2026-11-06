use super::support::{self, *};
use hf_application::process::IdentityPort;
use hf_domain::{
    fixture::{FixtureFault, Frame, Gate, Invitation, Prepared, QualifiedLedger, Stage},
    ownership::{Id, PreparedRequest},
};
use hf_interfaces::fixture::{decode, encode};
use hf_platform::{
    fixture::{gate_packet, read_frame, write_all},
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
};
use hf_storage::{Limits, RootHold, Sha256Digest, Store};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

pub(super) struct GatePeer {
    arrived: Arc<AtomicBool>,
    release: Option<SyncSender<()>>,
    join: Option<JoinHandle<Result<(), String>>>,
}
impl GatePeer {
    pub(super) fn start(
        listener: std::os::unix::net::UnixListener,
        stage: Stage,
        operation: Id,
        nonce: Id,
    ) -> Self {
        let arrived = Arc::new(AtomicBool::new(false));
        let observed = arrived.clone();
        let (release, receiver) = mpsc::sync_channel(1);
        let join =
            thread::spawn(move || serve(listener, stage, operation, nonce, observed, receiver));
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
        if let Some(sender) = self.release.take() {
            let _ = sender.send(());
        }
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        self.release();
        if self.join.is_none() {
            return Ok(());
        }
        let until = deadline(3);
        while self.join.as_ref().is_some_and(|join| !join.is_finished()) {
            if Instant::now() >= until {
                return Err("gate worker exceeded join bound".into());
            }
            thread::yield_now();
        }
        self.join
            .take()
            .ok_or("gate worker missing")?
            .join()
            .map_err(|_| "gate worker panicked".to_owned())?
    }
}
impl Drop for GatePeer {
    fn drop(&mut self) {
        self.release();
        if self.finish().is_err() && !thread::panicking() {
            std::process::abort();
        }
    }
}
fn serve(
    listener: std::os::unix::net::UnixListener,
    stage: Stage,
    operation: Id,
    nonce: Id,
    arrived: Arc<AtomicBool>,
    release: Receiver<()>,
) -> Result<(), String> {
    let until = deadline(9);
    let (stream, _) = loop {
        if release.try_recv().is_ok() {
            return Err("gate released before writer arrival".into());
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
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let packet = read_frame(&stream, until).map_err(|error| error.to_string())?;
    if packet != gate_packet(stage, &operation, &nonce) {
        return Err("gate packet mismatch".into());
    }
    arrived.store(true, Ordering::Release);
    release
        .recv_timeout(until.saturating_duration_since(Instant::now()))
        .map_err(|error| error.to_string())?;
    write_all(&stream, &packet, until).map_err(|error| error.to_string())
}
pub(super) fn gate_listener(scratch: &Path) -> (std::os::unix::net::UnixListener, PathBuf) {
    let path = scratch.join("fixed-owner-gate.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind finite owner gate");
    listener
        .set_nonblocking(true)
        .expect("nonblocking owner gate");
    (listener, path)
}
pub(super) fn spawn_owner(scratch: &Path) -> RetainedOwnedChild {
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: support::binary(),
            cwd: scratch.to_path_buf(),
            args: vec!["--internal-isolated-owner-fixture-v1".into()],
            environment: Vec::new(),
            capture_limit: 32768,
        },
        &MacIdentity,
        deadline(3),
    )
    .expect("spawn retained fixed owner")
}
pub(super) fn wait_arrived(peer: &GatePeer, until: Instant) {
    while !peer.arrived() {
        assert!(
            Instant::now() < until,
            "fixed writer did not reach explicit gate"
        );
        thread::yield_now();
    }
}
pub(super) fn start_fixed_owner(scratch: &Path) -> (RetainedOwnedChild, GatePeer) {
    let (root, request, ledger) = prepare_store(scratch);
    let (listener, socket) = gate_listener(scratch);
    let gate = Gate {
        stage: Stage::CaptureBeforeSync,
        path: socket.to_string_lossy().into_owned(),
    };
    let peer = GatePeer::start(
        listener,
        gate.stage,
        request.operation.clone(),
        support::id('7'),
    );
    let owner = owner_handshake(scratch, &root, request, ledger, gate);
    wait_arrived(&peer, deadline(5));
    (owner, peer)
}
fn prepare_store(scratch: &Path) -> (PathBuf, PreparedRequest, QualifiedLedger) {
    let root = scratch.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(3),
        hf_platform::lease::shared(),
    )
    .unwrap();
    drop(store);
    let source = RootHold::open(&root, deadline(3), hf_platform::lease::shared())
        .unwrap()
        .source_stamp()
        .clone();
    let ledger = support::ledger(scratch);
    let caller = MacIdentity
        .identity(std::process::id())
        .expect("actual caller identity");
    let pin = hf_platform::prepared_launch::observe_binary(
        &support::binary(),
        &Sha256Digest,
        deadline(3),
    )
    .unwrap();
    let request = support::request(caller, &scratch.to_string_lossy(), pin, ledger, source);
    let store = Store::open(
        &root,
        Limits::default(),
        deadline(3),
        hf_platform::lease::shared(),
    )
    .unwrap();
    store.ownership_prepare(&request, deadline(3)).unwrap();
    drop(store);
    (root, request, ledger)
}
fn owner_handshake(
    scratch: &Path,
    root: &Path,
    request: PreparedRequest,
    ledger: QualifiedLedger,
    gate: Gate,
) -> RetainedOwnedChild {
    let mut owner = spawn_owner(scratch);
    assert_eq!(
        owner.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    let Frame::Hello(owner_process) =
        decode(&owner.read_fixture_frame(deadline(3)).unwrap()).unwrap()
    else {
        panic!("owner hello required")
    };
    let invitation = Invitation {
        operation: request.operation.clone(),
        generation: request.generation.clone(),
        digest: request.digest.clone(),
        owner: support::id('6'),
        nonce: support::id('7'),
        process: owner_process,
        revision: 0,
    };
    owner
        .write_fixture_frame(
            &encode(&Frame::Invitation(invitation.clone())).unwrap(),
            deadline(3),
        )
        .unwrap();
    assert_eq!(
        decode(&owner.read_fixture_frame(deadline(3)).unwrap()).unwrap(),
        Frame::Ready(invitation.clone())
    );
    let prepared = Prepared {
        invitation,
        request,
        root: root.to_string_lossy().into_owned(),
        gate: Some(gate),
        ledger,
        fault: FixtureFault::None,
    };
    owner
        .write_fixture_frame(&encode(&Frame::Prepared(prepared)).unwrap(), deadline(3))
        .unwrap();
    owner.close_stdin();
    owner
}

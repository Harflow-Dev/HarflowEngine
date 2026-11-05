use super::*;
use hf_application::owner::{OwnerPort, OwnerSession};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot},
    ownership::{ClaimResult, Digest, Fence, Id, OwnerClaim, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
};
use hf_platform::process::{MacIdentity, RetainedOwnedChild, SpawnSpec};
use hf_storage::Store;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[path = "support/assertions.rs"]
mod assertions;
pub(super) use assertions::*;

pub(super) const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/d2_reader_writer.py"
);
pub(super) const SIZE: usize = 262_144;
pub(super) use crate::fixture::test_support::case_guard;

pub(super) struct Scratch(pub(super) PathBuf);
impl Scratch {
    pub(super) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = PathBuf::from("/private/tmp").join(format!(
            "hf-d2-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
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

pub(super) struct Worker {
    join: Option<JoinHandle<io::Result<WorkerResult>>>,
    pub(super) release: Sender<()>,
}
impl Worker {
    pub(super) fn join(&mut self) -> io::Result<WorkerResult> {
        let join = self.join.as_ref().expect("worker handle");
        let until = Instant::now() + Duration::from_secs(9);
        while !join.is_finished() && Instant::now() < until {
            std::thread::yield_now();
        }
        if !join.is_finished() {
            std::process::abort();
        }
        self.join.take().unwrap().join().expect("owner thread")
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.release.send(());
        let Some(join) = self.join.take() else { return };
        let until = Instant::now() + Duration::from_secs(8);
        while !join.is_finished() && Instant::now() < until {
            std::thread::yield_now();
        }
        if !join.is_finished() {
            std::process::abort();
        }
        let _ = join.join();
    }
}

pub(super) struct WorkerResult {
    pub(super) report: DrainReport,
    pub(super) session: OwnerSession<GatedStore>,
    pub(super) exit: ProcessExit,
    pub(super) final_observation: Option<hf_platform::process::ReaderFinal>,
}

/// Field order keeps the child/reader guard alive until raw and Store guards can unwind.
struct OwnerWork {
    owned: OwnerChild,
    raw: RawCapture,
    session: OwnerSession<GatedStore>,
    binding: CaptureBinding,
    until: Instant,
}
impl OwnerWork {
    fn run(mut self) -> io::Result<WorkerResult> {
        let mut gate = |_| Ok(());
        let result = drive(
            &mut self.owned,
            &mut self.raw,
            &mut self.session,
            Some(&self.binding),
            &mut gate,
            self.until,
        );
        let exit = self.owned.child.exit();
        let final_observation = self
            .owned
            .reader
            .as_ref()
            .and_then(|reader| reader.view().final_observation());
        self.owned.settle()?;
        Ok(WorkerResult {
            report: result?,
            session: self.session,
            exit,
            final_observation,
        })
    }
}

pub(super) struct CaptureGate {
    view: Mutex<Option<hf_platform::process::ReaderView>>,
    entered: Sender<hf_platform::process::ReaderView>,
    release: Mutex<Receiver<()>>,
}
impl CaptureGate {
    pub(super) fn new(
        entered: Sender<hf_platform::process::ReaderView>,
        release: Receiver<()>,
    ) -> Self {
        Self {
            view: Mutex::new(None),
            entered,
            release: Mutex::new(release),
        }
    }
    pub(super) fn set_view(&self, view: hf_platform::process::ReaderView) {
        *self.view.lock().unwrap() = Some(view);
    }
    fn wait(&self) -> io::Result<()> {
        let view = self
            .view
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| io::Error::other("test reader view absent"))?;
        self.entered
            .send(view)
            .map_err(|_| io::Error::other("test observer closed"))?;
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(6))
            .map_err(|_| io::Error::other("finite capture-begin gate expired"))
    }
}

pub(super) struct GatedStore {
    inner: Store,
    gate: Arc<CaptureGate>,
    fail_publish: AtomicBool,
    calls: Arc<CallCounts>,
}
#[derive(Default)]
pub(super) struct CallCounts {
    pub(super) begins: AtomicUsize,
    pub(super) publishes: AtomicUsize,
    pub(super) finishes: AtomicUsize,
}
impl GatedStore {
    pub(super) fn new(
        inner: Store,
        gate: Arc<CaptureGate>,
        fail_publish: bool,
    ) -> (Self, Arc<CallCounts>) {
        let calls = Arc::new(CallCounts::default());
        (
            Self {
                inner,
                gate,
                fail_publish: AtomicBool::new(fail_publish),
                calls: calls.clone(),
            },
            calls,
        )
    }
}
impl OwnerPort for GatedStore {
    type Error = hf_storage::Error;
    fn observe(&self, id: &Id, until: Instant) -> Result<Option<Snapshot>, Self::Error> {
        self.inner.ownership_observe(id, until)
    }
    fn claim(
        &self,
        id: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        until: Instant,
    ) -> Result<ClaimResult, Self::Error> {
        self.inner
            .ownership_claim(id, generation, revision, owner, until)
    }
    fn record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        until: Instant,
    ) -> Result<Snapshot, Self::Error> {
        self.inner.ownership_record_spawn(fence, child, until)
    }
    fn record_acceptance(&self, fence: &Fence, until: Instant) -> Result<Snapshot, Self::Error> {
        self.inner.ownership_record_acceptance(fence, until)
    }
    fn capture_begin(
        &self,
        binding: &CaptureBinding,
        until: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.calls.begins.fetch_add(1, Ordering::Relaxed);
        self.gate.wait().map_err(hf_storage::Error::Io)?;
        self.inner.capture_begin(binding, until)
    }
    fn capture_publish(
        &self,
        binding: &CaptureBinding,
        revision: u64,
        progress: &CaptureProgress,
        until: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.calls.publishes.fetch_add(1, Ordering::Relaxed);
        let result = self
            .inner
            .capture_publish(binding, revision, progress, until)?;
        if self.fail_publish.swap(false, Ordering::AcqRel) {
            return Err(hf_storage::Error::Io(io::Error::other(
                "injected lost publish reply",
            )));
        }
        Ok(result)
    }
    fn capture_finish(
        &self,
        binding: &CaptureBinding,
        revision: u64,
        exit: ProcessExit,
        until: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.calls.finishes.fetch_add(1, Ordering::Relaxed);
        self.inner.capture_finish(binding, revision, exit, until)
    }
}

pub(super) fn id(value: u8) -> Id {
    Id::new(format!("{value:02x}").repeat(16)).unwrap()
}
pub(super) fn request(store: &Store) -> PreparedRequest {
    PreparedRequest {
        operation: id(0x11),
        session: id(0x22),
        reservation: id(0x33),
        attempt: id(0x44),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        )
        .unwrap(),
        payload: Vec::new(),
    }
}
pub(super) fn child(witness: &Path) -> RetainedOwnedChild {
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args: vec![
                OsString::from("-I"),
                OsString::from("-S"),
                OsString::from(HELPER),
                witness.as_os_str().to_owned(),
            ],
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: Vec::new(),
            capture_limit: 1,
        },
        &MacIdentity,
        Instant::now() + Duration::from_secs(3),
    )
    .unwrap()
}
pub(super) fn wait_progress(view: &hf_platform::process::ReaderView, witness: &Path) {
    let until = Instant::now() + Duration::from_secs(6);
    loop {
        let progress = view.snapshot();
        if witness.exists()
            && progress.stdout.read == SIZE as u64
            && progress.stderr.read == SIZE as u64
            && progress.stdout.eof
            && progress.stderr.eof
        {
            assert_eq!(fs::read(witness).unwrap(), b"complete");
            assert!(
                progress.discard && progress.stdout.discarded > 0 && progress.stderr.discarded > 0
            );
            return;
        }
        assert!(Instant::now() < until, "held-gate reader progress deadline");
        std::thread::yield_now();
    }
}
pub(super) fn spawn_owner(
    owned: OwnerChild,
    raw: RawCapture,
    session: OwnerSession<GatedStore>,
    binding: CaptureBinding,
    release_guard: Sender<()>,
    owner_deadline: Instant,
) -> Worker {
    let work = OwnerWork {
        owned,
        raw,
        session,
        binding,
        until: owner_deadline,
    };
    let join = std::thread::spawn(move || work.run());
    Worker {
        join: Some(join),
        release: release_guard,
    }
}

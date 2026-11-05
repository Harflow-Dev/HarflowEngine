//! Internal bounded drainage; observations grant no process or caller authority.
use hf_domain::ownership::{Fence, Phase, Snapshot};
use hf_domain::process::ProcessIdentity;
use std::io;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureOrigin {
    pub fence: Fence,
    pub child: ProcessIdentity,
}
impl CaptureOrigin {
    pub fn from_spawn_observed(value: &Snapshot) -> io::Result<Self> {
        if value.phase != Phase::SpawnObserved {
            return Err(io::Error::other("capture origin is not SpawnObserved"));
        }
        Ok(Self {
            fence: value
                .fence()
                .ok_or_else(|| io::Error::other("capture owner absent"))?,
            child: value
                .child
                .clone()
                .ok_or_else(|| io::Error::other("capture child absent"))?,
        })
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReaderStreamStats {
    pub read: u64,
    pub enqueued: u64,
    pub discarded: u64,
    pub eof: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReaderProgress {
    pub stdout: ReaderStreamStats,
    pub stderr: ReaderStreamStats,
    pub discard: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReaderFinal {
    pub progress: ReaderProgress,
    pub stopped: bool,
    pub limited: bool,
}
#[derive(Debug)]
pub struct CaptureBatch {
    pub origin: CaptureOrigin,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub(super) struct Shared {
    pub progress: Mutex<ReaderProgress>,
    pub final_value: OnceLock<ReaderFinal>,
    pub stop: AtomicBool,
    pub discard: AtomicBool,
}
impl Shared {
    pub fn publish(&self, value: &ReaderProgress) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = value.clone();
    }
}
#[derive(Clone)]
pub struct ReaderView {
    pub(super) shared: Arc<Shared>,
}
impl ReaderView {
    pub fn snapshot(&self) -> ReaderProgress {
        self.shared
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn final_observation(&self) -> Option<ReaderFinal> {
        self.shared.final_value.get().cloned()
    }
}
pub struct PrivateFixtureCaptureReader {
    pub(super) origin: CaptureOrigin,
    pub(super) shared: Arc<Shared>,
    pub(super) receiver: Receiver<CaptureBatch>,
    pub(super) worker: Option<JoinHandle<ReaderFinal>>,
    pub(super) join_failed: bool,
}
impl PrivateFixtureCaptureReader {
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(|w| w.is_finished())
    }
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<CaptureBatch, std::sync::mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
    /// Join without consuming the FIFO, allowing post-join reconciliation.
    pub fn join_finished(&mut self) -> io::Result<ReaderFinal> {
        if !self.is_finished() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "reader unfinished",
            ));
        }
        if self.join_failed {
            return Err(io::Error::other("reader join previously failed"));
        }
        match self.worker.take().map(JoinHandle::join) {
            Some(Ok(value)) => Ok(value),
            Some(Err(_)) => {
                self.join_failed = true;
                Err(io::Error::other("reader panicked"))
            }
            None => self
                .shared
                .final_value
                .get()
                .cloned()
                .ok_or_else(|| io::Error::other("reader final absent")),
        }
    }
    pub fn origin(&self) -> &CaptureOrigin {
        &self.origin
    }
    pub fn view(&self) -> ReaderView {
        ReaderView {
            shared: self.shared.clone(),
        }
    }
    pub fn try_recv(&self) -> Result<CaptureBatch, TryRecvError> {
        self.receiver.try_recv()
    }
    pub fn discard_capture(&self) {
        self.shared.discard.store(true, Ordering::Release);
    }
    pub fn request_stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
    }
    /// Timeout leaves the unresolved worker owned by this handle.
    pub fn settle(&mut self, deadline: Instant) -> io::Result<ReaderFinal> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(1));
        while !self.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.join_finished()
    }
}
/// Defensive policy for the private fixed-helper reader only. The host must
/// settle it before releasing its Store/root guard. No general process Drop
/// policy is changed. An unresolved worker may not escape unwind by detachment.
impl Drop for PrivateFixtureCaptureReader {
    fn drop(&mut self) {
        self.request_stop();
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.worker.as_ref().is_some_and(|w| !w.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(worker) = self.worker.take() {
            if !worker.is_finished() {
                std::process::abort();
            }
            let _ = worker.join();
        }
    }
}

#![allow(dead_code)]

use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::Arc,
    time::Instant,
};

/// Test-only adapter for storage behavior tests. Cross-process production lock
/// qualification belongs to hf-platform and uses its real fcntl coordinator.
#[derive(Default)]
pub struct FileLeasePort;

struct HeldFile(File);
impl LeaseGuard for HeldFile {}

impl LeasePort for FileLeasePort {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        if Instant::now() >= deadline {
            return Err(LeaseError::DeadlineExceeded);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        if create {
            options.create(true);
        }
        let file = options.open(path).map_err(|_| LeaseError::Invalid)?;
        let result = match mode {
            LeaseMode::Shared => file.try_lock_shared(),
            LeaseMode::Exclusive => file.try_lock(),
        };
        result.map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => LeaseError::Busy,
            std::fs::TryLockError::Error(_) => LeaseError::Unavailable,
        })?;
        if Instant::now() >= deadline {
            let _ = file.unlock();
            return Err(LeaseError::DeadlineExceeded);
        }
        Ok(Box::new(HeldFile(file)))
    }
}

pub fn lease_port() -> Arc<dyn LeasePort> {
    Arc::new(FileLeasePort)
}

/// `FileLeasePort` that counts attempts on `io.lock` and can report each outcome.
#[derive(Default)]
pub struct CountingLease {
    inner: FileLeasePort,
    attempts: std::sync::atomic::AtomicUsize,
    busy: std::sync::atomic::AtomicUsize,
    acquired: std::sync::atomic::AtomicUsize,
    events: std::sync::Mutex<Option<std::sync::mpsc::Sender<IoEvent>>>,
}

/// One outcome of an exclusive `io.lock` attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoEvent {
    Busy,
    Acquired,
}

impl CountingLease {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    /// Attempts on `io.lock` so far.
    pub fn attempts(&self) -> usize {
        self.attempts.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn busy(&self) -> usize {
        self.busy.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn acquired(&self) -> usize {
        self.acquired.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// Each later `io.lock` outcome is sent to the returned receiver.
    pub fn events(&self) -> std::sync::mpsc::Receiver<IoEvent> {
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.events.lock().unwrap() = Some(sender);
        receiver
    }
    fn report(&self, event: IoEvent) {
        if let Some(sender) = self.events.lock().unwrap().as_ref() {
            let _ = sender.send(event);
        }
    }
}

impl LeasePort for CountingLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        use std::sync::atomic::Ordering::SeqCst;
        let counted = mode == LeaseMode::Exclusive && path.ends_with("io.lock");
        if counted {
            self.attempts.fetch_add(1, SeqCst);
        }
        let result = self.inner.acquire(path, mode, create, deadline);
        if counted {
            match &result {
                Ok(_) => {
                    self.acquired.fetch_add(1, SeqCst);
                    self.report(IoEvent::Acquired);
                }
                Err(LeaseError::Busy) => {
                    self.busy.fetch_add(1, SeqCst);
                    self.report(IoEvent::Busy);
                }
                Err(_) => {}
            }
        }
        result
    }
}

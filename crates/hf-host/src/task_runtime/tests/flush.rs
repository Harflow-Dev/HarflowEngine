//! A durability port that fails a chosen flush once armed.
use hf_application::durability::FlushPort;
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// Shared arming state: the `nth` flush after arming fails.
#[derive(Debug, Default)]
pub(super) struct Arm {
    armed: AtomicBool,
    nth: AtomicUsize,
    seen: AtomicUsize,
}
impl Arm {
    pub(super) fn arm(&self, nth: usize) {
        self.nth.store(nth, Ordering::Release);
        self.seen.store(0, Ordering::Release);
        self.armed.store(true, Ordering::Release);
    }
}
#[derive(Debug)]
pub(super) struct ArmedFlush {
    inner: Arc<dyn FlushPort>,
    arm: Arc<Arm>,
}
impl ArmedFlush {
    pub(super) fn new(inner: Arc<dyn FlushPort>, arm: Arc<Arm>) -> Self {
        Self { inner, arm }
    }
}
impl FlushPort for ArmedFlush {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        if self.arm.armed.load(Ordering::Acquire) {
            let seen = self.arm.seen.fetch_add(1, Ordering::AcqRel) + 1;
            if seen == self.arm.nth.load(Ordering::Acquire) {
                return Err(std::io::Error::other("injected flush failure"));
            }
        }
        self.inner.flush(file)
    }
}

//! Private fixed-helper cleanup guard; never permits a live reader to outlast root authority.
use super::*;
use hf_platform::process::PrivateFixtureCaptureReader;
#[cfg(test)]
#[path = "owned_tests.rs"]
mod tests;

pub(super) struct OwnerChild {
    pub child: RetainedOwnedChild,
    pub reader: Option<PrivateFixtureCaptureReader>,
    deadline: Instant,
    settled: bool,
}
impl OwnerChild {
    pub fn new(child: RetainedOwnedChild, deadline: Instant) -> Self {
        Self {
            child,
            reader: None,
            deadline,
            settled: false,
        }
    }
    pub fn settle(&mut self) -> io::Result<()> {
        if let Some(reader) = self.reader.as_mut() {
            reader.request_stop();
            reader.settle(self.deadline.min(Instant::now() + Duration::from_secs(1)))?;
        }
        cleanup(&mut self.child)?;
        self.settled = true;
        Ok(())
    }
}
impl Drop for OwnerChild {
    fn drop(&mut self) {
        if !self.settled && self.settle().is_err() {
            // Store/root are declared before this guard. Abort before their unwind.
            std::process::abort();
        }
    }
}

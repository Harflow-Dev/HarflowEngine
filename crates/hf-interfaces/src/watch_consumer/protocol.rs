//! Bounded consumer scheduling only; refreshes use public lifecycle operations.
use super::WatchDocument;
use crate::task_consumer::DecodeError;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshWork {
    /// Refresh public snapshot facts.
    Snapshot,
    /// Reconcile public lifecycle deadlines AND refresh snapshot facts.
    Reconcile,
}
#[derive(Default, Debug)]
pub struct ConsumerState {
    cursor: Option<String>,
    wait_in_flight: bool,
    refresh_busy: bool,
    pending: Option<RefreshWork>,
    baseline: bool,
}
impl ConsumerState {
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
    pub fn begin_wait(&mut self) -> Result<Option<String>, DecodeError> {
        if self.wait_in_flight {
            return Err(DecodeError::Field);
        }
        self.wait_in_flight = true;
        Ok(self.cursor.clone())
    }
    pub fn accept(&mut self, document: &WatchDocument) -> Result<(), DecodeError> {
        if !self.wait_in_flight {
            return Err(DecodeError::Field);
        }
        let state = document.state().ok_or(DecodeError::Payload)?;
        if !self.baseline && !["baseline", "reset", "unavailable"].contains(&state) {
            return Err(DecodeError::Field);
        }
        self.wait_in_flight = false;
        self.cursor = document.cursor().map(String::from);
        let work = match state {
            "baseline" | "reset" => {
                self.baseline = true;
                RefreshWork::Snapshot
            }
            "changed" => RefreshWork::Snapshot,
            "timeout" => RefreshWork::Reconcile,
            "unavailable" => {
                self.baseline = false;
                RefreshWork::Reconcile
            }
            _ => return Err(DecodeError::Field),
        };
        // Reconciliation includes snapshot refresh and cannot be erased by invalidation.
        self.pending = Some(if self.pending == Some(RefreshWork::Reconcile) {
            RefreshWork::Reconcile
        } else {
            work
        });
        Ok(())
    }
    pub fn wait_failed(&mut self) {
        self.wait_in_flight = false;
        self.cursor = None;
        self.baseline = false;
        self.pending = Some(RefreshWork::Reconcile);
    }
    pub fn take_refresh(&mut self) -> Option<RefreshWork> {
        if self.refresh_busy {
            return None;
        }
        let work = self.pending.take()?;
        self.refresh_busy = true;
        Some(work)
    }
    pub fn finish_refresh(&mut self) {
        self.refresh_busy = false;
    }
}

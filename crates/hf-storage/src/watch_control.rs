use hf_application::task_watch::{WatchControl, WatchError};
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};
/// Interruptible caller-owned control; spurious wakes never advance the poll clock.
#[derive(Clone, Default)]
pub struct RealWatchControl {
    state: Arc<CancelState>,
}
#[derive(Default)]
struct CancelState {
    cancelled: Mutex<bool>,
    wake: Condvar,
}
impl RealWatchControl {
    pub fn cancel(&self) -> Result<(), WatchError> {
        *self
            .state
            .cancelled
            .lock()
            .map_err(|_| WatchError::ControlUnavailable)? = true;
        self.state.wake.notify_all();
        Ok(())
    }
}
impl WatchControl for RealWatchControl {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn check(&self) -> Result<(), WatchError> {
        if *self
            .state
            .cancelled
            .lock()
            .map_err(|_| WatchError::ControlUnavailable)?
        {
            Err(WatchError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn wait_until(&self, target: Instant) -> Result<(), WatchError> {
        let mut cancelled = self
            .state
            .cancelled
            .lock()
            .map_err(|_| WatchError::ControlUnavailable)?;
        loop {
            if *cancelled {
                return Err(WatchError::Cancelled);
            }
            let remaining = target.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            (cancelled, _) = self
                .state
                .wake
                .wait_timeout(cancelled, remaining)
                .map_err(|_| WatchError::ControlUnavailable)?;
        }
    }
}

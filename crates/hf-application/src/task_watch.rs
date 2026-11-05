//! Authority-first internal fixture watch ports. No operating-system caller proof.
use crate::task_read::{Authority, TaskAuthorityPort};
use hf_domain::{
    task_read::AuthorizedScope,
    task_watch::{WatchCursor, WatchRequest, WatchResult},
};
use std::time::Instant;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchError {
    Preparing,
    Refused,
    InvalidWait,
    InvalidCursor,
    StateBusy,
    StateIncompatible,
    Cancelled,
    DeadlineExceeded,
    ControlUnavailable,
}
pub trait WatchCodecPort {
    fn decode(&self, raw: &str, expected_root: &str) -> Result<WatchCursor, WatchError>;
    fn encode(&self, cursor: &WatchCursor) -> Result<String, WatchError>;
}
pub trait WatchControl {
    fn now(&self) -> Instant;
    fn check(&self) -> Result<(), WatchError>;
    /// Wait until target or an error; non-cancelled spurious wakes must not return early.
    fn wait_until(&self, target: Instant) -> Result<(), WatchError>;
}
pub trait TaskWatchPort {
    fn execute(&self, request: &WatchRequest, deadline: Instant)
    -> Result<WatchResult, WatchError>;
}
pub struct TaskWatcher<A, R> {
    authority: A,
    repository: R,
}
impl<A: TaskAuthorityPort, R: TaskWatchPort> TaskWatcher<A, R> {
    pub fn new(authority: A, repository: R) -> Self {
        Self {
            authority,
            repository,
        }
    }
    pub fn watch(
        &self,
        request: &WatchRequest,
        deadline: Instant,
    ) -> Result<WatchResult, WatchError> {
        match self.authority.prepare() {
            Authority::Preparing => return Err(WatchError::Preparing),
            Authority::Authorized(AuthorizedScope::Global) => {}
            _ => return Err(WatchError::Refused),
        }
        if request.wait_seconds > 30 {
            return Err(WatchError::InvalidWait);
        }
        self.repository.execute(request, deadline)
    }
}

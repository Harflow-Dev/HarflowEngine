use crate::{Error, Limits, Store, binding::Binding};
use hf_application::lease::LeasePort;
use hf_application::task_watch::{TaskWatchPort, WatchCodecPort, WatchControl, WatchError};
use hf_domain::task_watch::{WatchCursor, WatchReason, WatchRequest, WatchResult, WatchState};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
const POLL: Duration = Duration::from_millis(50);
pub struct WatchSession<C, T> {
    leases: Arc<dyn LeasePort>,
    root: PathBuf,
    limits: Limits,
    codec: C,
    control: T,
}
impl<C: WatchCodecPort, T: WatchControl> WatchSession<C, T> {
    pub fn new(
        root: impl Into<PathBuf>,
        limits: Limits,
        codec: C,
        control: T,
        leases: Arc<dyn LeasePort>,
    ) -> Self {
        Self {
            leases,
            root: root.into(),
            limits,
            codec,
            control,
        }
    }
    fn check(&self, deadline: Instant) -> Result<(), WatchError> {
        self.control.check()?;
        if self.control.now() >= deadline {
            Err(WatchError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
    fn qualify(&self, deadline: Instant) -> Result<Option<Store>, WatchError> {
        let validated = Binding::validate_probe_path(&self.root);
        self.check(deadline)?;
        validated.map_err(root_error)?;
        let exists = self.root.try_exists();
        self.check(deadline)?;
        let exists = exists.map_err(|_| WatchError::ControlUnavailable)?;
        self.check(deadline)?;
        if !exists {
            return Ok(None);
        }
        let opened = Store::open_with(
            &self.root,
            self.limits,
            deadline,
            self.leases.clone(),
            crate::WriterOptions::suppressed(),
        );
        self.check(deadline)?;
        opened.map(Some).map_err(root_error)
    }
    fn result(
        &self,
        state: WatchState,
        cursor: &WatchCursor,
        reason: Option<WatchReason>,
        deadline: Instant,
    ) -> Result<WatchResult, WatchError> {
        let encoded = self.codec.encode(cursor)?;
        self.check(deadline)?;
        Ok(WatchResult {
            state,
            cursor: Some(encoded),
            reason,
        })
    }
    fn observe(
        &self,
        store: &Store,
        root: &str,
        deadline: Instant,
    ) -> Result<Option<WatchCursor>, WatchError> {
        self.check(deadline)?;
        let admission = deadline.min(self.control.now() + POLL);
        let revision = snapshot(store, admission);
        self.check(deadline)?;
        Ok(revision.ok().map(|revision| WatchCursor {
            root: root.to_owned(),
            epoch: store.identity().generation.clone(),
            revision,
        }))
    }
    fn wait(
        &self,
        store: &Store,
        root: &str,
        previous: WatchCursor,
        wait: u32,
        deadline: Instant,
    ) -> Result<WatchResult, WatchError> {
        let ceiling = deadline.min(self.control.now() + Duration::from_secs(u64::from(wait)));
        loop {
            let sampled = self.control.now();
            let Some(current) = self.observe(store, root, deadline)? else {
                return Ok(unavailable(WatchReason::ObservationUnavailable));
            };
            if current.epoch != previous.epoch {
                return self.result(
                    WatchState::Reset,
                    &current,
                    Some(WatchReason::EpochChanged),
                    deadline,
                );
            }
            if current.revision < previous.revision {
                return self.result(
                    WatchState::Reset,
                    &current,
                    Some(WatchReason::Discontinuity),
                    deadline,
                );
            }
            if current.revision > previous.revision {
                return self.result(WatchState::Changed, &current, None, deadline);
            }
            self.check(deadline)?;
            if self.control.now() >= ceiling {
                return self.result(WatchState::Timeout, &current, None, deadline);
            }
            self.control.wait_until((sampled + POLL).min(ceiling))?;
            self.check(deadline)?;
        }
    }
}
impl<C: WatchCodecPort, T: WatchControl> TaskWatchPort for WatchSession<C, T> {
    fn execute(
        &self,
        request: &WatchRequest,
        caller_deadline: Instant,
    ) -> Result<WatchResult, WatchError> {
        self.control.check()?;
        if request.wait_seconds > 30 {
            return Err(WatchError::InvalidWait);
        }
        let start = self.control.now();
        let wait = if request.cursor.is_some() {
            request.wait_seconds
        } else {
            0
        };
        let deadline = caller_deadline.min(start + Duration::from_secs(u64::from(wait) + 2));
        self.check(deadline)?;
        let qualified = match self.qualify(deadline.min(start + Duration::from_secs(2))) {
            Err(WatchError::ControlUnavailable) => {
                return Ok(unavailable(WatchReason::ObservationUnavailable));
            }
            other => other?,
        };
        let Some(store) = qualified else {
            return Ok(unavailable(WatchReason::StateMissing));
        };
        let root = root_key(&store);
        self.check(deadline)?;
        let root = match root {
            Ok(root) => root,
            Err(_) => return Ok(unavailable(WatchReason::ObservationUnavailable)),
        };
        if let Some(raw) = &request.cursor {
            let previous = self.codec.decode(raw, &root)?;
            if !previous.validate() || previous.root != root {
                return Err(WatchError::InvalidCursor);
            }
            self.check(deadline)?;
            return self.wait(&store, &root, previous, wait, deadline);
        }
        match self.observe(&store, &root, deadline)? {
            Some(cursor) => self.result(WatchState::Baseline, &cursor, None, deadline),
            None => Ok(unavailable(WatchReason::ObservationUnavailable)),
        }
    }
}
fn root_key(store: &Store) -> crate::Result<String> {
    let root = std::fs::canonicalize(store.binding.root())?;
    store.binding.check()?;
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    Ok(format!("{digest:x}")[..32].to_owned())
}
fn snapshot(store: &Store, deadline: Instant) -> crate::Result<i64> {
    let connection = store.admit(deadline)?;
    let revision = crate::watch_meta::revision(&connection)?;
    store.binding.check()?;
    crate::connection::check_deadline(deadline)?;
    Ok(revision)
}
fn unavailable(reason: WatchReason) -> WatchResult {
    WatchResult {
        state: WatchState::Unavailable,
        cursor: None,
        reason: Some(reason),
    }
}
fn root_error(error: Error) -> WatchError {
    match error {
        Error::Busy => WatchError::StateBusy,
        Error::DeadlineExceeded => WatchError::DeadlineExceeded,
        Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            WatchError::StateIncompatible
        }
        Error::Io(_) | Error::Sqlite(_) => WatchError::ControlUnavailable,
        _ => WatchError::StateIncompatible,
    }
}

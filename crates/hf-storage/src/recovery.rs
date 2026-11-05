//! Operation-scoped observations from qualified private copies only.
use crate::{Error, Limits, binding::Binding, connection, read_context::ReadContext};
use hf_application::recovery::{RecoveryError, RecoveryStorePort};
use hf_domain::{
    ownership::{Id, Phase},
    recovery::RecoveryRecord,
};
use std::{path::PathBuf, time::Instant};
pub struct RecoveryReadSession {
    root: PathBuf,
    scratch_parent: PathBuf,
    limits: Limits,
}
impl RecoveryReadSession {
    pub fn new(
        root: impl Into<PathBuf>,
        scratch_parent: impl Into<PathBuf>,
        limits: Limits,
    ) -> Self {
        Self {
            root: root.into(),
            scratch_parent: scratch_parent.into(),
            limits,
        }
    }
    fn snapshot(&self, operation: &Id, deadline: Instant) -> crate::Result<Option<RecoveryRecord>> {
        connection::check_deadline(deadline)?;
        Binding::validate_probe_path(&self.root)?;
        if !self.root.try_exists()? {
            return Ok(None);
        }
        let binding = Binding::capture(&self.root, true)?;
        let claim = std::fs::File::open(self.root.join("store.lock"))?;
        connection::claim(&claim, true)?;
        let _io = binding.io()?;
        crate::wal::with_qualified_copy(&binding, &self.scratch_parent, deadline, |db, identity| {
            crate::ownership_schema::check(db, &identity.generation)?;
            crate::capture_schema::check(db, &identity.generation)?;
            let context = ReadContext {
                identity,
                limits: self.limits,
            };
            let Some(ownership) = crate::ownership_read::load_context(&context, db, operation)?
            else {
                return Ok(None);
            };
            if ownership.phase == Phase::Released {
                return Err(Error::OwnershipConflict);
            }
            let capture = crate::capture_read::load_context(&context, db, operation)?;
            if let Some(c) = &capture
                && (ownership.phase != Phase::AcceptanceRecorded
                    || ownership.cancelled
                    || ownership.fence().as_ref() != Some(&c.binding.fence)
                    || ownership.child.as_ref() != Some(&c.binding.child))
            {
                return Err(Error::StaleOwnership);
            }
            let (events_count, events_digest) = crate::recovery_events::validate(
                db,
                operation,
                capture.as_ref(),
                self.limits,
                deadline,
            )?;
            let generation =
                Id::new(identity.generation.clone()).map_err(|_| Error::InvalidOwnership)?;
            let source = binding.source_stamp(generation)?;
            Ok(Some(RecoveryRecord {
                source,
                ownership,
                capture,
                events_count,
                events_digest,
            }))
        })
    }
}
impl RecoveryStorePort for RecoveryReadSession {
    fn read(
        &self,
        operation: &Id,
        deadline: Instant,
    ) -> Result<Option<RecoveryRecord>, RecoveryError> {
        let result = self.snapshot(operation, deadline).map_err(map);
        if Instant::now() >= deadline {
            return Err(RecoveryError::DeadlineExceeded);
        }
        result
    }
}
fn map(error: Error) -> RecoveryError {
    match error {
        Error::Busy => RecoveryError::Busy,
        Error::DeadlineExceeded => RecoveryError::DeadlineExceeded,
        Error::Replaced | Error::StaleOwnership => RecoveryError::Stale,
        Error::OwnershipConflict => RecoveryError::Refused,
        Error::InvalidOwnership
        | Error::LimitExceeded
        | Error::InvalidRoot
        | Error::Incompatible
        | Error::FutureSchema => RecoveryError::Invalid,
        _ => RecoveryError::Unavailable,
    }
}

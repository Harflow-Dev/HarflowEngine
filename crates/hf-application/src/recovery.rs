//! Bounded, injected read-only recovery observations.
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress},
    ownership::Id,
    process::ProcessIdentity,
    recovery::*,
};
use std::time::{Duration, Instant};
mod validate;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryError {
    Unavailable,
    Busy,
    Invalid,
    Stale,
    DeadlineExceeded,
    Refused,
}
pub trait RecoveryStorePort {
    fn read(
        &self,
        operation: &Id,
        deadline: Instant,
    ) -> Result<Option<RecoveryRecord>, RecoveryError>;
}
pub trait RecoveryIdentityPort {
    fn observe(
        &self,
        expected: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<ProcessEvidence, RecoveryError>;
}
pub trait RecoveryRawPort {
    fn observe(
        &self,
        binding: &CaptureBinding,
        progress: &CaptureProgress,
        deadline: Instant,
    ) -> Result<RawEvidence, RecoveryError>;
}
pub struct RecoveryInspector<S, I, R> {
    store: S,
    identity: I,
    raw: R,
}
impl<S: RecoveryStorePort, I: RecoveryIdentityPort, R: RecoveryRawPort> RecoveryInspector<S, I, R> {
    pub fn new(store: S, identity: I, raw: R) -> Self {
        Self {
            store,
            identity,
            raw,
        }
    }
    pub fn inspect(
        &self,
        operation: &Id,
        deadline: Instant,
    ) -> Result<Option<RecoveryReport>, RecoveryError> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(2));
        check(deadline)?;
        let first = self.store.read(operation, deadline);
        check(deadline)?;
        let first = first?;
        let Some(record) = first else {
            return Ok(None);
        };
        validate::record(&record, operation)?;
        let owner = record
            .ownership
            .owner
            .as_ref()
            .map(|o| self.process(&o.process, deadline))
            .transpose()?;
        let child = record
            .ownership
            .child
            .as_ref()
            .map(|p| self.process(p, deadline))
            .transpose()?;
        let raw = self.raw(&record, deadline)?;
        let second = self.store.read(operation, deadline);
        check(deadline)?;
        let second = second?;
        if second.as_ref() != Some(&record) {
            return Err(RecoveryError::Stale);
        }
        let last_raw = self.raw(&record, deadline)?;
        if last_raw != raw {
            return Err(RecoveryError::Stale);
        }
        let capture = validate::assessment(record.capture.as_ref(), raw.as_ref());
        Ok(Some(RecoveryReport {
            record,
            owner,
            child,
            raw,
            capture,
            reservation: ReservationDisposition::Retained,
        }))
    }
    fn process(
        &self,
        expected: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<ProcessEvidence, RecoveryError> {
        let evidence = self.identity.observe(expected, deadline);
        check(deadline)?;
        let evidence = evidence?;
        validate::process(&evidence, expected)?;
        Ok(evidence)
    }
    fn raw(
        &self,
        record: &RecoveryRecord,
        deadline: Instant,
    ) -> Result<Option<RawEvidence>, RecoveryError> {
        let evidence = record
            .capture
            .as_ref()
            .map(|c| self.raw.observe(&c.binding, &c.progress, deadline))
            .transpose();
        check(deadline)?;
        evidence
    }
}
fn check(deadline: Instant) -> Result<(), RecoveryError> {
    if Instant::now() >= deadline {
        Err(RecoveryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

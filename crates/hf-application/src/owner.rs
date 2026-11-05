//! Retained single-operation internal ownership; no caller or process authority.
use hf_domain::{
    capture::*,
    ownership::*,
    process::{ProcessExit, ProcessIdentity},
};
use std::time::Instant;
mod capture;

pub trait OwnerPort {
    type Error;
    fn observe(&self, operation: &Id, deadline: Instant) -> Result<Option<Snapshot>, Self::Error>;
    fn claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        deadline: Instant,
    ) -> Result<ClaimResult, Self::Error>;
    fn record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<Snapshot, Self::Error>;
    fn record_acceptance(&self, fence: &Fence, deadline: Instant) -> Result<Snapshot, Self::Error>;
    fn capture_begin(
        &self,
        binding: &CaptureBinding,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Self::Error>;
    fn capture_publish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        next: &CaptureProgress,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Self::Error>;
    fn capture_finish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        exit: ProcessExit,
        deadline: Instant,
    ) -> Result<CaptureSnapshot, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum OwnerError<E> {
    Poisoned,
    InvalidState,
    InvalidResponse,
    NoGrant,
    Port(E),
}

pub struct OwnerSession<P> {
    port: P,
    request: PreparedRequest,
    owner: OwnerClaim,
    expected_revision: u64,
    snapshot: Option<Snapshot>,
    capture: Option<CaptureSnapshot>,
    poisoned: bool,
}
impl<P: OwnerPort> OwnerSession<P> {
    /// Holds the port before any I/O or claim; construction grants no authority.
    pub fn new(
        port: P,
        request: PreparedRequest,
        owner: OwnerClaim,
        expected_revision: u64,
    ) -> Self {
        Self {
            port,
            request,
            owner,
            expected_revision,
            snapshot: None,
            capture: None,
            poisoned: false,
        }
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }
    pub fn capture(&self) -> Option<&CaptureSnapshot> {
        self.capture.as_ref()
    }
    pub fn poison(&mut self) {
        self.poisoned = true;
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    fn refuse<T>(&mut self, error: OwnerError<P::Error>) -> Result<T, OwnerError<P::Error>> {
        self.poison();
        Err(error)
    }
    fn healthy(&self) -> Result<(), OwnerError<P::Error>> {
        if self.poisoned {
            Err(OwnerError::Poisoned)
        } else {
            Ok(())
        }
    }
    fn response<T>(&mut self, result: Result<T, P::Error>) -> Result<T, OwnerError<P::Error>> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => self.refuse(OwnerError::Port(error)),
        }
    }
    pub fn claim(&mut self, deadline: Instant) -> Result<(), OwnerError<P::Error>> {
        self.healthy()?;
        if self.snapshot.is_some()
            || self.request.payload.len() > 16384
            || self.expected_revision != 0
        {
            return self.refuse(OwnerError::InvalidState);
        }
        let result = self.port.observe(&self.request.operation, deadline);
        let observed = self.response(result)?;
        let expected = Snapshot {
            request: self.request.clone(),
            phase: Phase::Prepared,
            revision: self.expected_revision,
            cancelled: false,
            owner: None,
            child: None,
        };
        if observed.as_ref() != Some(&expected) {
            return self.refuse(OwnerError::InvalidResponse);
        }
        let result = self.port.claim(
            &self.request.operation,
            &self.request.generation,
            self.expected_revision,
            &self.owner,
            deadline,
        );
        let granted = match self.response(result)? {
            ClaimResult::Granted(value) => value,
            ClaimResult::NoGrant(_) => return self.refuse(OwnerError::NoGrant),
        };
        let mut expected = expected;
        expected.owner = Some(self.owner.clone());
        expected.phase = Phase::Claimed;
        expected.revision += 1;
        self.save_snapshot(granted, expected)
    }
    fn save_snapshot(
        &mut self,
        actual: Snapshot,
        expected: Snapshot,
    ) -> Result<(), OwnerError<P::Error>> {
        if actual != expected {
            return self.refuse(OwnerError::InvalidResponse);
        }
        self.snapshot = Some(actual);
        Ok(())
    }
    fn current(&mut self, phase: Phase) -> Result<Snapshot, OwnerError<P::Error>> {
        self.healthy()?;
        match self.snapshot.as_ref() {
            Some(value) if value.phase == phase && self.capture.is_none() => Ok(value.clone()),
            _ => self.refuse(OwnerError::InvalidState),
        }
    }
    pub fn record_child(
        &mut self,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let mut expected = self.current(Phase::Claimed)?;
        if child == &self.owner.process {
            return self.refuse(OwnerError::InvalidState);
        }
        let Some(fence) = expected.fence() else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result = self.port.record_spawn(&fence, child, deadline);
        let actual = self.response(result)?;
        expected.phase = Phase::SpawnObserved;
        expected.revision += 1;
        expected.child = Some(child.clone());
        self.save_snapshot(actual, expected)
    }
    pub fn accept(&mut self, deadline: Instant) -> Result<(), OwnerError<P::Error>> {
        let mut expected = self.current(Phase::SpawnObserved)?;
        let Some(fence) = expected.fence() else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result = self.port.record_acceptance(&fence, deadline);
        let actual = self.response(result)?;
        expected.phase = Phase::AcceptanceRecorded;
        expected.revision += 1;
        self.save_snapshot(actual, expected)
    }
}

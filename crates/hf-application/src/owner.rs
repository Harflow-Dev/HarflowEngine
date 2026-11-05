//! Retained single-operation internal ownership; no caller or process authority.
use hf_domain::{
    capture::*,
    ownership::*,
    process::{ProcessExit, ProcessIdentity},
};
use std::time::Instant;
pub(crate) mod capture;
mod state;

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
    state: SharedOwnerState,
}
#[derive(Clone, Debug)]
pub(crate) struct SharedOwnerState {
    pub(crate) request: PreparedRequest,
    pub(crate) owner: OwnerClaim,
    pub(crate) expected_revision: u64,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) capture: Option<CaptureSnapshot>,
    pub(crate) poisoned: bool,
}
impl SharedOwnerState {
    pub(crate) fn new(request: PreparedRequest, owner: OwnerClaim, expected_revision: u64) -> Self {
        Self {
            request,
            owner,
            expected_revision,
            snapshot: None,
            capture: None,
            poisoned: false,
        }
    }
    pub(crate) fn prepared(&self) -> Snapshot {
        Snapshot {
            request: self.request.clone(),
            phase: Phase::Prepared,
            revision: self.expected_revision,
            cancelled: false,
            owner: None,
            child: None,
        }
    }
    pub(crate) fn accept_snapshot(&mut self, actual: Snapshot, expected: Snapshot) -> bool {
        if actual != expected {
            self.poisoned = true;
            return false;
        }
        self.snapshot = Some(actual);
        true
    }
    pub(crate) fn accept_capture(
        &mut self,
        actual: CaptureSnapshot,
        expected: CaptureSnapshot,
    ) -> bool {
        if actual != expected {
            self.poisoned = true;
            return false;
        }
        self.capture = Some(actual);
        true
    }
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
            state: SharedOwnerState::new(request, owner, expected_revision),
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.state.snapshot.as_ref()
    }
    pub fn capture(&self) -> Option<&CaptureSnapshot> {
        self.state.capture.as_ref()
    }
    pub fn poison(&mut self) {
        self.state.poisoned = true;
    }
    pub fn is_poisoned(&self) -> bool {
        self.state.poisoned
    }
    fn refuse<T>(&mut self, error: OwnerError<P::Error>) -> Result<T, OwnerError<P::Error>> {
        self.poison();
        Err(error)
    }
    fn healthy(&self) -> Result<(), OwnerError<P::Error>> {
        if self.state.poisoned {
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
        if self.state.snapshot.is_some()
            || self.state.request.payload.len() > 16384
            || self.state.expected_revision != 0
        {
            return self.refuse(OwnerError::InvalidState);
        }
        let result = self.port.observe(&self.state.request.operation, deadline);
        let observed = self.response(result)?;
        let expected = self.state.prepared();
        if observed.as_ref() != Some(&expected) {
            return self.refuse(OwnerError::InvalidResponse);
        }
        let result = self.port.claim(
            &self.state.request.operation,
            &self.state.request.generation,
            self.state.expected_revision,
            &self.state.owner,
            deadline,
        );
        let granted = match self.response(result)? {
            ClaimResult::Granted(value) => value,
            ClaimResult::NoGrant(_) => return self.refuse(OwnerError::NoGrant),
        };
        let Some(expected) = self
            .state
            .next_snapshot(&hf_domain::owner_runtime::StorageCommand::Claim)
        else {
            return self.refuse(OwnerError::InvalidState);
        };
        self.save_snapshot(granted, expected)
    }
    fn save_snapshot(
        &mut self,
        actual: Snapshot,
        expected: Snapshot,
    ) -> Result<(), OwnerError<P::Error>> {
        if !self.state.accept_snapshot(actual, expected) {
            return Err(OwnerError::InvalidResponse);
        }
        Ok(())
    }

    fn current(&mut self, phase: Phase) -> Result<Snapshot, OwnerError<P::Error>> {
        self.healthy()?;
        match self.state.snapshot.as_ref() {
            Some(value) if value.phase == phase && self.state.capture.is_none() => {
                Ok(value.clone())
            }
            _ => self.refuse(OwnerError::InvalidState),
        }
    }
    pub fn record_child(
        &mut self,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let expected = self.current(Phase::Claimed)?;
        if child == &self.state.owner.process {
            return self.refuse(OwnerError::InvalidState);
        }
        let Some(fence) = expected.fence() else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result = self.port.record_spawn(&fence, child, deadline);
        let actual = self.response(result)?;
        let Some(expected) =
            self.state
                .next_snapshot(&hf_domain::owner_runtime::StorageCommand::RecordChild(
                    child.clone(),
                ))
        else {
            return self.refuse(OwnerError::InvalidState);
        };
        self.save_snapshot(actual, expected)
    }
    pub fn accept(&mut self, deadline: Instant) -> Result<(), OwnerError<P::Error>> {
        let expected = self.current(Phase::SpawnObserved)?;
        let Some(fence) = expected.fence() else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result = self.port.record_acceptance(&fence, deadline);
        let actual = self.response(result)?;
        let Some(expected) = self
            .state
            .next_snapshot(&hf_domain::owner_runtime::StorageCommand::Accept)
        else {
            return self.refuse(OwnerError::InvalidState);
        };
        self.save_snapshot(actual, expected)
    }
}

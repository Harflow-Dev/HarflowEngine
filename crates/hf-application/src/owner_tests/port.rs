use super::support::*;
use crate::owner::OwnerPort;
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Observe,
    Claim,
    Spawn,
    Accept,
    Begin,
    Publish,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    None,
    Error(Call),
    Malformed(Call),
    NoGrant,
    PreparedMismatch(u8),
}

pub(crate) struct FakePort {
    pub(crate) state: RefCell<Snapshot>,
    pub(crate) capture: RefCell<Option<CaptureSnapshot>>,
    pub(crate) calls: Rc<RefCell<Vec<Call>>>,
    pub(crate) fault: Fault,
    pub(crate) dropped: Rc<RefCell<bool>>,
}

impl Drop for FakePort {
    fn drop(&mut self) {
        *self.dropped.borrow_mut() = true;
    }
}

impl FakePort {
    fn call(&self, call: Call) -> Result<(), &'static str> {
        self.calls.borrow_mut().push(call);
        if self.fault == Fault::Error(call) {
            Err("ambiguous committed write")
        } else {
            Ok(())
        }
    }
    fn malformed(&self, call: Call) -> bool {
        self.fault == Fault::Malformed(call)
    }
}

impl OwnerPort for FakePort {
    type Error = &'static str;
    fn observe(&self, operation: &Id, _: Instant) -> Result<Option<Snapshot>, Self::Error> {
        self.call(Call::Observe)?;
        let mut value = self.state.borrow().clone();
        assert_eq!(operation, &value.request.operation);
        if let Fault::PreparedMismatch(field) = self.fault {
            match field {
                0 => value.request.operation = id('8'),
                1 => value.request.session = id('8'),
                2 => value.request.reservation = id('8'),
                3 => value.request.attempt = id('8'),
                4 => value.request.generation = id('8'),
                5 => value.request.digest = digest('b'),
                6 => value.request.payload.push(0),
                7 => value.revision += 1,
                8 => value.phase = Phase::Claimed,
                _ => value.cancelled = true,
            }
        }
        Ok(Some(value))
    }
    fn claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        _: Instant,
    ) -> Result<ClaimResult, Self::Error> {
        self.call(Call::Claim)?;
        let current = self.state.borrow();
        assert_eq!(operation, &current.request.operation);
        assert_eq!(generation, &current.request.generation);
        assert_eq!(revision, current.revision);
        assert_eq!(owner, &super::support::owner());
        drop(current);
        if self.fault == Fault::NoGrant {
            return Ok(ClaimResult::NoGrant(self.state.borrow().clone()));
        }
        let mut value = self.state.borrow().clone();
        value.owner = Some(owner.clone());
        value.phase = Phase::Claimed;
        value.revision += 1;
        *self.state.borrow_mut() = value.clone();
        if self.malformed(Call::Claim) {
            value.request.payload.push(0);
        }
        Ok(ClaimResult::Granted(value))
    }
    fn record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        _: Instant,
    ) -> Result<Snapshot, Self::Error> {
        self.call(Call::Spawn)?;
        let mut value = self.state.borrow().clone();
        assert_eq!(value.fence(), Some(fence.clone()));
        assert_ne!(child, &value.owner.as_ref().unwrap().process);
        value.phase = Phase::SpawnObserved;
        value.revision += 1;
        value.child = Some(child.clone());
        *self.state.borrow_mut() = value.clone();
        if self.malformed(Call::Spawn) {
            value.revision += 1;
        }
        Ok(value)
    }
    fn record_acceptance(&self, fence: &Fence, _: Instant) -> Result<Snapshot, Self::Error> {
        self.call(Call::Accept)?;
        let mut value = self.state.borrow().clone();
        assert_eq!(value.fence(), Some(fence.clone()));
        value.phase = Phase::AcceptanceRecorded;
        value.revision += 1;
        *self.state.borrow_mut() = value.clone();
        if self.malformed(Call::Accept) {
            value.revision += 1;
        }
        Ok(value)
    }
    fn capture_begin(
        &self,
        binding: &CaptureBinding,
        _: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.call(Call::Begin)?;
        let value = CaptureSnapshot {
            binding: binding.clone(),
            progress: CaptureProgress {
                revision: 0,
                stdout: 0,
                stderr: 0,
                stdout_eof: false,
                stderr_eof: false,
                loss: false,
            },
            exit: ProcessExit::Unobserved,
        };
        *self.capture.borrow_mut() = Some(value.clone());
        let mut result = value;
        if self.malformed(Call::Begin) {
            result.binding.child = process(99);
        }
        Ok(result)
    }
    fn capture_publish(
        &self,
        binding: &CaptureBinding,
        revision: u64,
        next: &CaptureProgress,
        _: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.call(Call::Publish)?;
        let mut value = self.capture.borrow().clone().unwrap();
        assert_eq!(&value.binding, binding);
        assert_eq!(value.progress.revision, revision);
        value.progress = next.clone();
        *self.capture.borrow_mut() = Some(value.clone());
        if self.malformed(Call::Publish) {
            value.progress.revision += 1;
        }
        Ok(value)
    }
    fn capture_finish(
        &self,
        binding: &CaptureBinding,
        revision: u64,
        exit: ProcessExit,
        _: Instant,
    ) -> Result<CaptureSnapshot, Self::Error> {
        self.call(Call::Finish)?;
        let mut value = self.capture.borrow().clone().unwrap();
        assert_eq!(&value.binding, binding);
        assert_eq!(value.progress.revision, revision);
        value.progress.revision += 1;
        value.exit = exit;
        *self.capture.borrow_mut() = Some(value.clone());
        if self.malformed(Call::Finish) {
            value.exit = ProcessExit::Unobserved;
        }
        Ok(value)
    }
}

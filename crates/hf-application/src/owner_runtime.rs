//! Split-phase private storage coordination. No I/O, retries or adoption.
use crate::owner::SharedOwnerState;
use hf_domain::{capture::*, owner_runtime::*, ownership::*};
use std::time::Instant;
mod transition;
pub trait DigestPort {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinatorError {
    Poisoned,
    Pending,
    InvalidState,
    InvalidAck,
    Expired,
}
#[derive(Clone, Debug)]
pub struct PendingCommand {
    pub identity: CommandIdentity,
    pub command: StorageCommand,
    pub deadline: Instant,
    pub sent: bool,
    expected_snapshot: Option<Snapshot>,
    expected_capture: Option<CaptureSnapshot>,
}
pub struct StorageCoordinator {
    state: SharedOwnerState,
    binding: Digest,
    next_sequence: u64,
    pending: Option<PendingCommand>,
    bootstrapped: bool,
    raw_files: Option<RawIdentities>,
    closed: bool,
}
impl StorageCoordinator {
    /// Only a fresh prepared request can enter; a stored row cannot construct this state.
    pub fn new(request: PreparedRequest, owner: OwnerClaim, binding: Digest) -> Self {
        Self {
            state: SharedOwnerState::new(request, owner, 0),
            binding,
            next_sequence: 1,
            pending: None,
            bootstrapped: false,
            raw_files: None,
            closed: false,
        }
    }
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
    pub fn binding_digest(&self) -> &Digest {
        &self.binding
    }
    pub fn raw_files(&self) -> Option<&RawIdentities> {
        self.raw_files.as_ref()
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.state.snapshot.as_ref()
    }
    pub fn capture(&self) -> Option<&CaptureSnapshot> {
        self.state.capture.as_ref()
    }
    pub fn pending(&self) -> Option<&PendingCommand> {
        self.pending.as_ref()
    }
    pub fn is_poisoned(&self) -> bool {
        self.state.poisoned
    }
    pub fn poison(&mut self) {
        self.state.poisoned = true;
    }
    pub fn begin(
        &mut self,
        command: StorageCommand,
        digest: Digest,
        deadline: Instant,
        now: Instant,
    ) -> Result<&PendingCommand, CoordinatorError> {
        if self.state.poisoned {
            return Err(CoordinatorError::Poisoned);
        }
        if self.pending.is_some() {
            return Err(CoordinatorError::Pending);
        }
        if deadline <= now {
            self.poison();
            return Err(CoordinatorError::Expired);
        }
        let (snapshot, capture) = self.project(&command)?;
        let pending = PendingCommand {
            identity: CommandIdentity {
                sequence: self.next_sequence,
                binding_digest: self.binding.clone(),
                command_digest: digest,
            },
            command,
            deadline,
            sent: false,
            expected_snapshot: snapshot,
            expected_capture: capture,
        };
        self.pending = Some(pending);
        self.pending.as_ref().ok_or(CoordinatorError::InvalidState)
    }
    pub fn send_complete(
        &mut self,
        identity: &CommandIdentity,
        now: Instant,
    ) -> Result<(), CoordinatorError> {
        if self.state.poisoned {
            return Err(CoordinatorError::Poisoned);
        }
        match self.pending.as_mut() {
            Some(p) if &p.identity == identity && now < p.deadline && !p.sent => {
                p.sent = true;
                Ok(())
            }
            _ => {
                self.poison();
                Err(CoordinatorError::InvalidState)
            }
        }
    }
    pub fn check_deadline(&mut self, now: Instant) -> Result<(), CoordinatorError> {
        if self.pending.as_ref().is_some_and(|p| now >= p.deadline) {
            self.poison();
            return Err(CoordinatorError::Expired);
        }
        if self.state.poisoned {
            Err(CoordinatorError::Poisoned)
        } else {
            Ok(())
        }
    }
    pub fn acknowledge(&mut self, ack: StorageAck, now: Instant) -> Result<(), CoordinatorError> {
        self.check_deadline(now)?;
        let Some(p) = self.pending.as_ref() else {
            self.poison();
            return Err(CoordinatorError::InvalidAck);
        };
        let revision = p
            .expected_capture
            .as_ref()
            .map(|c| c.progress.revision)
            .or_else(|| p.expected_snapshot.as_ref().map(|s| s.revision));
        if !p.sent
            || ack.identity != p.identity
            || ack.disposition != AckDisposition::Committed
            || ack.snapshot != p.expected_snapshot
            || ack.capture != p.expected_capture
            || ack.revision != revision
            || !self.valid_raw_ack(&p.command, ack.raw_files.as_ref())
            || self.next_sequence == u64::MAX
        {
            self.poison();
            return Err(CoordinatorError::InvalidAck);
        }
        let Some(p) = self.pending.take() else {
            return Err(CoordinatorError::InvalidState);
        };
        if let Some(s) = p.expected_snapshot {
            self.state.snapshot = Some(s);
        }
        self.state.capture = p.expected_capture;
        match p.command {
            StorageCommand::Bootstrap(_) => self.bootstrapped = true,
            StorageCommand::CreateRaw => self.raw_files = ack.raw_files,
            StorageCommand::Close => self.closed = true,
            _ => {}
        }
        self.next_sequence += 1;
        Ok(())
    }
    fn valid_raw_ack(&self, command: &StorageCommand, raw: Option<&RawIdentities>) -> bool {
        if matches!(command, StorageCommand::CreateRaw) {
            self.raw_files.is_none() && raw.is_some_and(RawIdentities::is_valid)
        } else {
            self.raw_files.as_ref() == raw
        }
    }
}

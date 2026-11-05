//! One pending exchange with incremental inherited-pipe I/O.
use hf_application::owner_runtime::StorageCoordinator;
use hf_domain::{owner_runtime::*, ownership::Digest};
use hf_interfaces::owner_storage_codec::{self as codec, Message};
use hf_platform::process::PrivateStorageIo;
use hf_storage::Sha256Digest;
use std::time::{Duration, Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ack {
    Bootstrap,
    Claim,
    RecordChild,
    CreateRaw,
    Accept,
    CaptureBegin,
    Append,
    KnownLoss,
    Finish,
    Close,
}
pub(super) struct Actor {
    pub(super) io: PrivateStorageIo,
    pub(super) coordinator: StorageCoordinator,
    until: Instant,
    kind: Option<Ack>,
}
impl Actor {
    pub(super) fn new(
        io: PrivateStorageIo,
        coordinator: StorageCoordinator,
        until: Instant,
    ) -> Self {
        Self {
            io,
            coordinator,
            until,
            kind: None,
        }
    }
    pub(super) fn submit(&mut self, command: StorageCommand) -> Result<(), String> {
        let kind = kind(&command);
        let message = Message::Command(command);
        let bytes = codec::encode(
            &message,
            self.coordinator.next_sequence(),
            self.coordinator.binding_digest(),
            &Sha256Digest,
        )
        .map_err(error)?;
        let mut raw = [0; 32];
        raw.copy_from_slice(&bytes[50..82]);
        let Message::Command(command) = message else {
            return Err("command shape".into());
        };
        self.coordinator
            .begin(
                command,
                Digest::from_sha256_bytes(raw),
                self.until,
                Instant::now(),
            )
            .map_err(error)?;
        self.kind = Some(kind);
        if let Err(e) = self.io.queue_frame(bytes) {
            self.poison();
            return Err(error(e));
        }
        Ok(())
    }
    pub(super) fn poison(&mut self) {
        self.coordinator.poison();
        self.io.close_input();
    }
    pub(super) fn idle(&self) -> bool {
        self.coordinator.pending().is_none()
    }
    pub(super) fn step(&mut self) -> Result<Option<Ack>, String> {
        let result = self.step_inner();
        if result.is_err() {
            self.poison();
            self.io.take_frame();
        }
        result
    }
    fn step_inner(&mut self) -> Result<Option<Ack>, String> {
        let now = Instant::now();
        self.io
            .step(now + Duration::from_millis(20))
            .map_err(error)?;
        if self.coordinator.is_poisoned() {
            self.io.take_frame();
            return Ok(None);
        }
        self.coordinator
            .check_deadline(Instant::now())
            .map_err(error)?;
        if self.io.send_complete()
            && let Some(p) = self.coordinator.pending()
            && !p.sent
        {
            let identity = p.identity.clone();
            self.coordinator
                .send_complete(&identity, Instant::now())
                .map_err(error)?;
        }
        let Some(bytes) = self.io.take_frame() else {
            return Ok(None);
        };
        let frame = codec::decode(&bytes, &Sha256Digest).map_err(error)?;
        let Message::Ack(ack) = frame.message else {
            return Err("writer sent command".into());
        };
        self.coordinator
            .acknowledge(*ack, Instant::now())
            .map_err(error)?;
        let kind = self.kind.take();
        if kind == Some(Ack::Close) {
            self.io.close_input();
        }
        Ok(kind)
    }
}
fn kind(c: &StorageCommand) -> Ack {
    match c {
        StorageCommand::Bootstrap(_) => Ack::Bootstrap,
        StorageCommand::Claim => Ack::Claim,
        StorageCommand::RecordChild(_) => Ack::RecordChild,
        StorageCommand::CreateRaw => Ack::CreateRaw,
        StorageCommand::Accept => Ack::Accept,
        StorageCommand::CaptureBegin(_) => Ack::CaptureBegin,
        StorageCommand::Append { .. } => Ack::Append,
        StorageCommand::KnownLoss => Ack::KnownLoss,
        StorageCommand::Finish(_) => Ack::Finish,
        StorageCommand::Close => Ack::Close,
    }
}
fn error(e: impl std::fmt::Debug) -> String {
    format!("owner exchange: {e:?}")
}

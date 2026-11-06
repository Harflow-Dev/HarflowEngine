//! Read-only exact capture acceptance. No execution or release authority is returned.
use crate::{
    owner_runtime::DigestPort,
    recovery::{RecoveryError, RecoveryStorePort},
};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress},
    fixture::Invitation,
    ownership::{Phase, PreparedRequest},
    prepared_launch::*,
    process::ProcessExit,
    recovery::{RecoveryRecord, SourceStamp},
};
use std::time::{Duration, Instant};
pub trait StreamDigestPort {
    fn update(&mut self, bytes: &[u8]) -> Result<(), RecoveryError>;
    fn finish(self) -> Result<hf_domain::ownership::Digest, RecoveryError>;
}
pub trait StreamingDigestFactory {
    type Stream: StreamDigestPort;
    fn start(&self, limit: usize) -> Result<Self::Stream, RecoveryError>;
}
pub trait ReceiptDeliveryPort {
    fn receive(
        &self,
        invitation: &Invitation,
        deadline: Instant,
    ) -> Result<Option<CaptureAccepted>, RecoveryError>;
}
pub trait ExactBinaryPort {
    fn observe(&self, logical_path: &str, deadline: Instant) -> Result<BinaryPin, RecoveryError>;
}
pub trait ExactRawPort {
    fn read_exact(
        &self,
        binding: &CaptureBinding,
        progress: &CaptureProgress,
        deadline: Instant,
    ) -> Result<RawBytes, RecoveryError>;
}
pub struct CaptureContext<'a> {
    pub invitation: &'a Invitation,
    pub request: &'a PreparedRequest,
    pub source: &'a SourceStamp,
}
pub struct ExactCaptureReader<S, R, H> {
    store: S,
    raw: R,
    hash: H,
}
impl<S: RecoveryStorePort, R: ExactRawPort, H: DigestPort> ExactCaptureReader<S, R, H> {
    pub fn new(store: S, raw: R, hash: H) -> Self {
        Self { store, raw, hash }
    }
    pub fn read(
        &self,
        context: CaptureContext<'_>,
        delivery: &impl ReceiptDeliveryPort,
        deadline: Instant,
    ) -> Result<CapturedSource, RecoveryError> {
        context_shape(&context, &self.hash)?;
        check(deadline)?;
        let receipt = delivery
            .receive(context.invitation, deadline)?
            .ok_or(RecoveryError::Refused)?;
        validate(&context, &receipt, &self.hash)?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(2));
        check(deadline)?;
        let first = self
            .store
            .read(&context.request.operation, deadline)?
            .ok_or(RecoveryError::Stale)?;
        check(deadline)?;
        matching(&first, &receipt)?;
        let bytes = self.raw.read_exact(
            &receipt.capture.binding,
            &receipt.capture.progress,
            deadline,
        )?;
        check(deadline)?;
        if bytes.stdout.len() as u64 != receipt.capture.progress.stdout
            || bytes.stderr.len() as u64 != receipt.capture.progress.stderr
            || self.hash.digest_parts(&[&bytes.stdout]) != receipt.stdout_digest
            || self.hash.digest_parts(&[&bytes.stderr]) != receipt.stderr_digest
        {
            return Err(RecoveryError::Stale);
        }
        let last = self.store.read(&context.request.operation, deadline)?;
        check(deadline)?;
        if last.as_ref() != Some(&first) {
            return Err(RecoveryError::Stale);
        }
        let stdout_len = (bytes.stdout.len() as u64).to_be_bytes();
        let stderr_len = (bytes.stderr.len() as u64).to_be_bytes();
        let framed_digest = self.hash.digest_parts(&[
            b"HFCS\x01",
            &stdout_len,
            &bytes.stdout,
            &stderr_len,
            &bytes.stderr,
        ]);
        Ok(CapturedSource {
            receipt,
            bytes,
            record: first,
            framed_digest,
        })
    }
}
fn check(deadline: Instant) -> Result<(), RecoveryError> {
    if Instant::now() >= deadline {
        Err(RecoveryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
fn matching(record: &RecoveryRecord, receipt: &CaptureAccepted) -> Result<(), RecoveryError> {
    if record.source != receipt.source
        || record.ownership != receipt.ownership
        || record.capture.as_ref() != Some(&receipt.capture)
        || record.events_count == 0
        || record.events_count > 128
    {
        return Err(RecoveryError::Stale);
    }
    Ok(())
}
fn validate(
    context: &CaptureContext<'_>,
    receipt: &CaptureAccepted,
    hash: &impl DigestPort,
) -> Result<(), RecoveryError> {
    let s = &receipt.ownership;
    let i = &receipt.invitation;
    let c = &receipt.capture;
    let p = &c.progress;
    if context.invitation != i
        || context.request != &s.request
        || context.source != &receipt.source
        || s.request.payload.len() > 16384
        || hash.digest_parts(&[&s.request.payload]) != s.request.digest
        || receipt.source.generation != s.request.generation
        || i.operation != s.request.operation
        || i.generation != s.request.generation
        || i.digest != s.request.digest
        || i.revision != 0
        || s.phase != Phase::AcceptanceRecorded
        || s.cancelled
        || s.revision != 3
        || s.owner.as_ref() != Some(&c.binding.fence.owner)
        || s.child.as_ref() != Some(&c.binding.child)
        || c.binding.fence.operation != i.operation
        || c.binding.fence.generation != i.generation
        || c.binding.fence.attempt != s.request.attempt
        || c.binding.fence.revision != s.revision
        || c.binding.fence.owner.owner != i.owner
        || c.binding.fence.owner.nonce != i.nonce
        || c.binding.fence.owner.process != i.process
        || p.stdout > 65536
        || p.stderr > 65536
        || !p.stdout_eof
        || !p.stderr_eof
        || p.loss
        || (p.revision == 0 || p.revision > 128)
        || c.exit != ProcessExit::Code(0)
        || c.binding.child == c.binding.fence.owner.process
        || c.binding.stdout == c.binding.stderr
        || c.binding.stdout.inode == 0
        || c.binding.stderr.inode == 0
    {
        return Err(RecoveryError::Invalid);
    }
    Ok(())
}

fn context_shape(c: &CaptureContext<'_>, hash: &impl DigestPort) -> Result<(), RecoveryError> {
    let r = c.request;
    let i = c.invitation;
    if r.payload.len() > 16384
        || !r.payload.starts_with(b"HFPL\x01")
        || hash.digest_parts(&[&r.payload]) != r.digest
        || r.operation != i.operation
        || r.generation != i.generation
        || r.digest != i.digest
        || c.source.generation != r.generation
        || i.revision != 0
    {
        return Err(RecoveryError::Invalid);
    }
    for id in [
        &c.source.root,
        &c.source.database,
        &c.source.marker,
        &c.source.store_lock,
        &c.source.io_lock,
    ] {
        if id.inode == 0 {
            return Err(RecoveryError::Invalid);
        }
    }
    Ok(())
}

//! Incremental immutable payload transfer. A tick produces at most one bounded frame.
use super::Error;
use hf_application::{owner_runtime::DigestPort, task_runtime::InitialIntentValidator};
use hf_domain::{
    ownership::PreparedRequest,
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_interfaces::task_runtime::{
    BlobChunk, BlobTransfer, CHUNK_BYTES, decode_chunk, encode_chunk,
};

pub struct IntentSender {
    intent: LaunchIntentCreate,
    stream: u8,
    offset: usize,
}
impl IntentSender {
    pub fn new(intent: LaunchIntentCreate) -> Result<Self, Error> {
        if intent.metadata.is_empty()
            || intent.metadata.len() > 16384
            || intent.prompt.len() > 4194304
        {
            return Err(Error::Refused);
        }
        Ok(Self {
            intent,
            stream: 0,
            offset: 0,
        })
    }
    /// Returns a complete length-prefixed channel packet. Exhaustion grants no execution.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            let (bytes, digest) = match self.stream {
                0 => (&self.intent.metadata[..], &self.intent.metadata_digest),
                1 => (self.intent.prompt.as_bytes(), &self.intent.prompt_digest),
                _ => return Ok(None),
            };
            if self.offset == bytes.len() {
                self.stream += 1;
                self.offset = 0;
                continue;
            }
            let end = (self.offset + CHUNK_BYTES).min(bytes.len());
            let chunk = BlobChunk {
                offset: self.offset as u32,
                total: bytes.len() as u32,
                digest: digest.clone(),
                bytes: bytes[self.offset..end].to_vec(),
            };
            let encoded = encode_chunk(&chunk).map_err(|_| Error::Refused)?;
            let mut frame = ((encoded.len() + 1) as u32).to_be_bytes().to_vec();
            frame.push(self.stream);
            frame.extend_from_slice(&encoded);
            self.offset = end;
            return Ok(Some(frame));
        }
    }
}
pub struct IntentReceiver {
    request: PreparedRequest,
    invitation: ProductionInvitation,
    metadata: Option<BlobTransfer>,
    prompt: Option<BlobTransfer>,
    metadata_total: Option<u32>,
    prompt_total: Option<u32>,
    metadata_received: u32,
    prompt_received: u32,
    poisoned: bool,
}
impl IntentReceiver {
    pub fn new(request: &PreparedRequest) -> Result<Self, Error> {
        let v = hf_interfaces::task_runtime::decode_invitation(&request.payload)
            .map_err(|_| Error::Refused)?;
        if hf_storage::Sha256Digest.digest_parts(&[&request.payload]) != request.digest
            || v.binding.operation != request.operation
            || v.binding.attempt != request.attempt
            || v.binding.generation != request.generation
            || v.reservation != request.reservation
            || v.ownership_session != request.session
        {
            return Err(Error::Refused);
        }
        Ok(Self {
            request: request.clone(),
            invitation: v,
            metadata: None,
            prompt: None,
            metadata_total: None,
            prompt_total: None,
            metadata_received: 0,
            prompt_received: 0,
            poisoned: false,
        })
    }
    /// Accepts a channel packet including its length prefix. Any violation is sticky.
    pub fn push(&mut self, frame: &[u8]) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        self.poisoned = true;
        self.push_inner(frame)?;
        self.poisoned = false;
        Ok(())
    }
    fn push_inner(&mut self, frame: &[u8]) -> Result<(), Error> {
        if frame.len() < 6
            || frame.len() > 24580
            || u32::from_be_bytes(frame[..4].try_into().map_err(|_| Error::Refused)?) as usize
                != frame.len() - 4
        {
            return Err(Error::Refused);
        }
        let chunk = decode_chunk(&frame[5..]).map_err(|_| Error::Refused)?;
        let (slot, total, received, digest, limit) = match frame[4] {
            0 if self.prompt.is_none() => (
                &mut self.metadata,
                &mut self.metadata_total,
                &mut self.metadata_received,
                &self.invitation.metadata_digest,
                16384,
            ),
            1 if self.metadata_total == Some(self.metadata_received) => (
                &mut self.prompt,
                &mut self.prompt_total,
                &mut self.prompt_received,
                &self.invitation.prompt_digest,
                4194304,
            ),
            _ => return Err(Error::Refused),
        };
        if &chunk.digest != digest {
            return Err(Error::Refused);
        }
        if slot.is_none() {
            *slot = Some(
                BlobTransfer::new(chunk.total, digest.clone(), limit)
                    .map_err(|_| Error::Refused)?,
            );
            *total = Some(chunk.total);
        }
        slot.as_mut()
            .ok_or(Error::Refused)?
            .push(&chunk)
            .map_err(|_| Error::Refused)?;
        *received += chunk.bytes.len() as u32;
        Ok(())
    }
    /// Storage must still rehash and validate the completed intent before T0/T1.
    pub fn finish(self) -> Result<LaunchIntentCreate, Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        let (metadata, metadata_digest) = self
            .metadata
            .ok_or(Error::Refused)?
            .finish()
            .map_err(|_| Error::Refused)?;
        let (prompt, prompt_digest) = match self.prompt {
            Some(p) => p.finish().map_err(|_| Error::Refused)?,
            None => (Vec::new(), self.invitation.prompt_digest),
        };
        let intent = LaunchIntentCreate {
            task: self.invitation.binding.task,
            metadata,
            metadata_digest,
            prompt: String::from_utf8(prompt).map_err(|_| Error::Refused)?,
            prompt_digest,
        };
        if hf_storage::Sha256Digest.digest_parts(&[&intent.metadata]) != intent.metadata_digest
            || hf_storage::Sha256Digest.digest_parts(&[intent.prompt.as_bytes()])
                != intent.prompt_digest
        {
            return Err(Error::Refused);
        }
        hf_interfaces::task_runtime::CanonicalLaunchValidator
            .validate(&self.request, &intent)
            .map_err(|_| Error::Refused)?;
        Ok(intent)
    }
}

//! Bounded private storage framing; authenticated inherited-channel qualification is separate.
use hf_application::owner_runtime::DigestPort;
use hf_domain::{owner_runtime::*, ownership::*};
mod binding;
mod bootstrap;
mod read;
pub use binding::participant_digest;
mod write;
pub const MAX_FRAME: usize = 24576;
const HEADER: usize = 82;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    Length,
    Shape,
    Value,
    Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Command(StorageCommand),
    Ack(Box<StorageAck>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub identity: CommandIdentity,
    pub message: Message,
}
/// Construct one allocation and hash borrowed header/body segments, excluding digest and length.
pub fn encode(
    message: &Message,
    sequence: u64,
    binding: &Digest,
    hash: &impl DigestPort,
) -> Result<Vec<u8>, CodecError> {
    if sequence == 0 {
        return Err(CodecError::Value);
    }
    if let Message::Ack(a) = message
        && (a.identity.sequence != sequence || &a.identity.binding_digest != binding)
    {
        return Err(CodecError::Value);
    }
    let mut w = write::Writer(Vec::with_capacity(MAX_FRAME));
    w.0.extend_from_slice(&[0; HEADER]);
    w.0[4..9].copy_from_slice(b"HFOR\x01");
    w.0[9] = match message {
        Message::Command(_) => 1,
        Message::Ack(_) => 2,
    };
    w.0[10..18].copy_from_slice(&sequence.to_be_bytes());
    w.0[18..50].copy_from_slice(&raw_digest(binding)?);
    w.message(message)?;
    if w.0.len() > MAX_FRAME {
        return Err(CodecError::Length);
    }
    let digest = hash.digest_parts(&[&w.0[4..50], &w.0[82..]]);
    w.0[50..82].copy_from_slice(&raw_digest(&digest)?);
    let length = (w.0.len() - 4) as u32;
    w.0[..4].copy_from_slice(&length.to_be_bytes());
    Ok(w.0)
}
pub fn decode(bytes: &[u8], hash: &impl DigestPort) -> Result<Frame, CodecError> {
    if bytes.len() < HEADER || bytes.len() > MAX_FRAME {
        return Err(CodecError::Length);
    }
    let mut r = read::Reader { bytes, at: 0 };
    if r.u32()? as usize != bytes.len() - 4 {
        return Err(CodecError::Length);
    }
    if r.take(5)? != b"HFOR\x01" {
        return Err(CodecError::Shape);
    }
    let tag = r.byte()?;
    let sequence = r.u64()?;
    if sequence == 0 {
        return Err(CodecError::Value);
    }
    let binding_digest = r.digest()?;
    let command_digest = r.digest()?;
    if hash.digest_parts(&[&bytes[4..50], &bytes[82..]]) != command_digest {
        return Err(CodecError::Digest);
    }
    let identity = CommandIdentity {
        sequence,
        binding_digest,
        command_digest,
    };
    let message = r.message(tag)?;
    if let Message::Ack(a) = &message
        && (a.identity.sequence != identity.sequence
            || a.identity.binding_digest != identity.binding_digest)
    {
        return Err(CodecError::Value);
    }
    if let Message::Command(StorageCommand::Bootstrap(b)) = &message
        && participant_digest(&b.participant, hash) != identity.binding_digest
    {
        return Err(CodecError::Digest);
    }
    if r.at != bytes.len() {
        return Err(CodecError::Shape);
    }
    Ok(Frame { identity, message })
}
fn raw_digest(d: &Digest) -> Result<[u8; 32], CodecError> {
    let mut raw = [0; 32];
    for (i, pair) in d.as_str().as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| CodecError::Value)?;
        raw[i] = u8::from_str_radix(text, 16).map_err(|_| CodecError::Value)?;
    }
    Ok(raw)
}

/// Read identity from an already encoded frame without allocating/decoding its payload.
/// This validates framing and digest only; it is not a typed-message decoder or authority.
pub fn encoded_identity(
    bytes: &[u8],
    hash: &impl DigestPort,
) -> Result<CommandIdentity, CodecError> {
    if bytes.len() < HEADER || bytes.len() > MAX_FRAME {
        return Err(CodecError::Length);
    }
    let mut r = read::Reader { bytes, at: 0 };
    if r.u32()? as usize != bytes.len() - 4 {
        return Err(CodecError::Length);
    }
    if r.take(5)? != b"HFOR\x01" || !matches!(r.byte()?, 1 | 2) {
        return Err(CodecError::Shape);
    }
    let sequence = r.u64()?;
    if sequence == 0 {
        return Err(CodecError::Value);
    }
    let binding_digest = r.digest()?;
    let command_digest = r.digest()?;
    if hash.digest_parts(&[&bytes[4..50], &bytes[82..]]) != command_digest {
        return Err(CodecError::Digest);
    }
    Ok(CommandIdentity {
        sequence,
        binding_digest,
        command_digest,
    })
}

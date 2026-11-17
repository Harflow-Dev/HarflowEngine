//! Counted, ordered transfer for one immutable blob. A partial transfer is never executable.
use hf_application::task_runtime::InvalidIntent;
use hf_domain::ownership::Digest;
pub const CHUNK_BYTES: usize = 16384;
pub const FRAME_BYTES: usize = 24576;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobChunk {
    pub offset: u32,
    pub total: u32,
    pub digest: Digest,
    pub bytes: Vec<u8>,
}
/// Trusted storage rehashes the completed bytes before accepting them.
pub struct BlobTransfer {
    total: u32,
    digest: Digest,
    bytes: Vec<u8>,
}
impl BlobTransfer {
    pub fn new(total: u32, digest: Digest, limit: usize) -> Result<Self, InvalidIntent> {
        if total as usize > limit || limit > 4194304 {
            return Err(InvalidIntent);
        }
        Ok(Self {
            total,
            digest,
            bytes: Vec::new(),
        })
    }
    pub fn push(&mut self, chunk: &BlobChunk) -> Result<(), InvalidIntent> {
        if chunk.total != self.total
            || chunk.digest != self.digest
            || chunk.offset as usize != self.bytes.len()
            || chunk.bytes.is_empty()
            || chunk.bytes.len() > CHUNK_BYTES
            || self
                .bytes
                .len()
                .checked_add(chunk.bytes.len())
                .is_none_or(|n| n > self.total as usize)
        {
            return Err(InvalidIntent);
        }
        self.bytes.extend_from_slice(&chunk.bytes);
        Ok(())
    }
    pub fn finish(self) -> Result<(Vec<u8>, Digest), InvalidIntent> {
        if self.bytes.len() != self.total as usize {
            return Err(InvalidIntent);
        }
        Ok((self.bytes, self.digest))
    }
}
pub fn encode_chunk(chunk: &BlobChunk) -> Result<Vec<u8>, InvalidIntent> {
    if chunk.bytes.is_empty()
        || chunk.bytes.len() > CHUNK_BYTES
        || (chunk.offset as usize)
            .checked_add(chunk.bytes.len())
            .is_none_or(|n| n > chunk.total as usize)
        || chunk.total > 4194304
    {
        return Err(InvalidIntent);
    }
    let mut frame = b"HFTC\x01".to_vec();
    frame.extend_from_slice(&chunk.offset.to_be_bytes());
    frame.extend_from_slice(&chunk.total.to_be_bytes());
    frame.extend_from_slice(chunk.digest.as_str().as_bytes());
    frame.extend_from_slice(&(chunk.bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&chunk.bytes);
    Ok(frame)
}
pub fn decode_chunk(frame: &[u8]) -> Result<BlobChunk, InvalidIntent> {
    if frame.len() < 81 || frame.len() > FRAME_BYTES || &frame[..5] != b"HFTC\x01" {
        return Err(InvalidIntent);
    }
    let number = |at: usize| -> Result<u32, InvalidIntent> {
        Ok(u32::from_be_bytes(
            frame[at..at + 4].try_into().map_err(|_| InvalidIntent)?,
        ))
    };
    let digest = std::str::from_utf8(&frame[13..77]).map_err(|_| InvalidIntent)?;
    let chunk = BlobChunk {
        offset: number(5)?,
        total: number(9)?,
        digest: Digest::new(digest.to_owned()).map_err(|_| InvalidIntent)?,
        bytes: frame[81..].to_vec(),
    };
    if number(77)? as usize != chunk.bytes.len() || encode_chunk(&chunk)? != frame {
        return Err(InvalidIntent);
    }
    Ok(chunk)
}

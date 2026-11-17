//! Digest-bound environment transfer; only complete immutable bytes are executable.
use super::{Error, io};
use hf_application::owner_runtime::DigestPort;
use hf_domain::ownership::Digest;
use hf_interfaces::task_runtime::{
    BlobChunk, BlobTransfer, CHUNK_BYTES, decode_chunk, decode_environment, encode_chunk,
    encode_environment,
};
use hf_platform::task_runtime::channel::Channel;
pub struct EnvironmentSender {
    bytes: Vec<u8>,
    digest: Digest,
    at: usize,
}
impl EnvironmentSender {
    pub fn new(values: impl AsRef<[(String, String)]>) -> Result<Self, Error> {
        let bytes = encode_environment(values.as_ref()).map_err(|_| Error::Refused)?;
        let digest = hf_storage::Sha256Digest.digest_parts(&[&bytes]);
        Ok(Self {
            bytes,
            digest,
            at: 0,
        })
    }
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.at == self.bytes.len() {
            return Ok(None);
        }
        let end = (self.at + CHUNK_BYTES).min(self.bytes.len());
        let chunk = BlobChunk {
            offset: self.at as u32,
            total: self.bytes.len() as u32,
            digest: self.digest.clone(),
            bytes: self.bytes[self.at..end].to_vec(),
        };
        let mut bytes = vec![2];
        bytes.extend(encode_chunk(&chunk).map_err(|_| Error::Refused)?);
        self.at = end;
        Ok(Some(io::frame(bytes)))
    }
}
pub fn receive_environment(
    channel: &mut Channel,
    expected: &Digest,
) -> Result<Vec<(String, String)>, Error> {
    let mut transfer = None;
    let mut received = 0;
    loop {
        let frame = io::receive(channel, io::deadline())?;
        if frame.get(4) != Some(&2) {
            return Err(Error::Refused);
        }
        let chunk =
            decode_chunk(frame.get(5..).ok_or(Error::Refused)?).map_err(|_| Error::Refused)?;
        if &chunk.digest != expected || chunk.total > 65536 {
            return Err(Error::Refused);
        }
        if transfer.is_none() {
            transfer = Some(
                BlobTransfer::new(chunk.total, expected.clone(), 65536)
                    .map_err(|_| Error::Refused)?,
            );
        }
        transfer
            .as_mut()
            .ok_or(Error::Refused)?
            .push(&chunk)
            .map_err(|_| Error::Refused)?;
        received += chunk.bytes.len();
        if received == chunk.total as usize {
            break;
        }
    }
    let (bytes, _) = transfer
        .ok_or(Error::Refused)?
        .finish()
        .map_err(|_| Error::Refused)?;
    if hf_storage::Sha256Digest.digest_parts(&[&bytes]) != *expected {
        return Err(Error::Refused);
    }
    decode_environment(&bytes).map_err(|_| Error::Refused)
}

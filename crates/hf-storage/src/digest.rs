//! Pure SHA-256 adapter over exact caller-provided byte parts.
use hf_application::owner_runtime::DigestPort;
use hf_domain::ownership::Digest;
use sha2::{Digest as _, Sha256};
pub struct Sha256Digest;
impl DigestPort for Sha256Digest {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        let mut hash = Sha256::new();
        for part in parts {
            hash.update(part);
        }
        Digest::from_sha256_bytes(hash.finalize().into())
    }
}

/// Incremental, constant-memory hashing for bounded diagnostic inputs.
/// This object performs no I/O and conveys no executable/admission authority.
pub struct Sha256Stream {
    hash: Sha256,
    length: usize,
    limit: usize,
    failed: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestSizeLimit;
impl Sha256Stream {
    pub fn with_limit(limit: usize) -> Result<Self, DigestSizeLimit> {
        if limit > 512 * 1024 * 1024 {
            return Err(DigestSizeLimit);
        }
        Ok(Self {
            hash: Sha256::new(),
            length: 0,
            limit,
            failed: false,
        })
    }
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), DigestSizeLimit> {
        let next = self.length.checked_add(bytes.len());
        if self.failed || next.is_none_or(|n| n > self.limit) {
            self.failed = true;
            return Err(DigestSizeLimit);
        }
        self.hash.update(bytes);
        self.length = next.ok_or(DigestSizeLimit)?;
        Ok(())
    }
    pub fn finish(self) -> Result<Digest, DigestSizeLimit> {
        if self.failed {
            return Err(DigestSizeLimit);
        }
        Ok(Digest::from_sha256_bytes(self.hash.finalize().into()))
    }
}

impl hf_application::prepared_launch::StreamDigestPort for Sha256Stream {
    fn update(&mut self, bytes: &[u8]) -> Result<(), hf_application::recovery::RecoveryError> {
        self.update(bytes)
            .map_err(|_| hf_application::recovery::RecoveryError::Refused)
    }
    fn finish(self) -> Result<Digest, hf_application::recovery::RecoveryError> {
        self.finish()
            .map_err(|_| hf_application::recovery::RecoveryError::Refused)
    }
}
impl hf_application::prepared_launch::StreamingDigestFactory for Sha256Digest {
    type Stream = Sha256Stream;
    fn start(&self, limit: usize) -> Result<Self::Stream, hf_application::recovery::RecoveryError> {
        Sha256Stream::with_limit(limit)
            .map_err(|_| hf_application::recovery::RecoveryError::Refused)
    }
}

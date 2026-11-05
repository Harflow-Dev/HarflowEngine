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

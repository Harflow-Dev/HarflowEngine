//! The digest port: a hash over exact caller-provided byte parts.
use hf_domain::ownership::Digest;

pub trait DigestPort {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest;
}

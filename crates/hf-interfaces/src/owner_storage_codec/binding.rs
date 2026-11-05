use super::*;
use hf_domain::{capture::FileIdentity, process::ProcessIdentity, recovery::SourceStamp};
/// Exact immutable participant bytes, including source identities and prepared payload digest.
/// Request digest covers static policy, caller/grant and steering context in preparation.
pub fn participant_digest(p: &StorageParticipant, hash: &impl DigestPort) -> Digest {
    let mut bytes = Vec::with_capacity(1024);
    bytes.extend_from_slice(b"harflow.owner-runtime.participant\0\x01");
    source(&mut bytes, &p.source);
    for id in [
        &p.operation,
        &p.attempt,
        &p.owner.owner,
        &p.owner.nonce,
        &p.channel_nonce,
    ] {
        bytes.extend_from_slice(id.as_str().as_bytes());
    }
    for d in [
        &p.request_digest,
        &p.configuration_digest,
        &p.profile_digest,
    ] {
        bytes.extend_from_slice(d.as_str().as_bytes());
    }
    process(&mut bytes, &p.owner.process);
    process(&mut bytes, &p.writer);
    hash.digest_parts(&[&bytes])
}
fn source(out: &mut Vec<u8>, s: &SourceStamp) {
    out.extend_from_slice(s.generation.as_str().as_bytes());
    for f in [&s.root, &s.database, &s.marker, &s.store_lock, &s.io_lock] {
        file(out, f);
    }
    out.extend_from_slice(&5_u64.to_be_bytes());
}
fn file(out: &mut Vec<u8>, f: &FileIdentity) {
    out.extend_from_slice(&f.device.to_be_bytes());
    out.extend_from_slice(&f.inode.to_be_bytes());
}
fn process(out: &mut Vec<u8>, p: &ProcessIdentity) {
    out.extend_from_slice(&p.pid().to_be_bytes());
    out.extend_from_slice(&p.start_seconds().to_be_bytes());
    out.extend_from_slice(&p.start_microseconds().to_be_bytes());
    out.extend_from_slice(p.boot_uuid().as_bytes());
}

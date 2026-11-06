//! Tagged private launch preparation. This codec grants no execution authority.
use crate::owner_preparation::{self, PreparationError};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    owner_policy::{FixtureAssignment, PreparationEnvelope},
    owner_runtime::StorageParticipant,
    ownership::*,
    prepared_launch::*,
};
pub(crate) mod wire;
#[derive(Clone, Debug, PartialEq)]
pub enum DecodedPreparation {
    Legacy(Box<PreparationEnvelope>),
    ClaudeQualification(Box<hf_domain::claude_qualification::ClaudeQualification>),
    FixedProtocol(Box<PreparedLaunch>),
}
impl DecodedPreparation {
    pub fn policy(&self) -> &PreparationEnvelope {
        match self {
            Self::Legacy(p) => p,
            Self::ClaudeQualification(p) => &p.policy,
            Self::FixedProtocol(p) => &p.policy,
        }
    }
}
pub fn configuration_digest(
    value: &PreparedLaunch,
    hash: &impl DigestPort,
) -> Result<Digest, PreparationError> {
    check(value)?;
    let mut p = value.policy.clone();
    p.configuration_digest = Digest::from_sha256_bytes([0; 32]);
    let policy = owner_preparation::encode(&p)?;
    let fields = wire::fields(value)?;
    Ok(hash.digest_parts(&[
        b"HFPL-config\x01",
        &(policy.len() as u32).to_be_bytes(),
        &policy,
        &fields,
    ]))
}
pub fn encode(value: &PreparedLaunch, hash: &impl DigestPort) -> Result<Vec<u8>, PreparationError> {
    check(value)?;
    if configuration_digest(value, hash)? != value.policy.configuration_digest {
        return Err(PreparationError::Binding);
    }
    let policy = owner_preparation::encode(&value.policy)?;
    let mut out = b"HFPL\x01".to_vec();
    out.extend_from_slice(&(policy.len() as u32).to_be_bytes());
    out.extend_from_slice(&policy);
    out.extend_from_slice(&wire::fields(value)?);
    if out.len() > owner_preparation::MAX_PAYLOAD {
        return Err(PreparationError::Shape);
    }
    Ok(out)
}
pub fn decode(
    request: &PreparedRequest,
    hash: &impl DigestPort,
) -> Result<PreparedLaunch, PreparationError> {
    if request.payload.len() > owner_preparation::MAX_PAYLOAD
        || hash.digest_parts(&[&request.payload]) != request.digest
    {
        return Err(PreparationError::Binding);
    }
    let (policy, fields) = wire::split(&request.payload)?;
    let mut embedded = request.clone();
    embedded.payload = policy.to_vec();
    embedded.digest = hash.digest_parts(&[policy]);
    let policy = owner_preparation::decode(&embedded, hash)?;
    let value = wire::decode(policy, fields)?;
    check(&value)?;
    if configuration_digest(&value, hash)? != value.policy.configuration_digest {
        return Err(PreparationError::Binding);
    }
    Ok(value)
}
pub fn decode_preparation(
    request: &PreparedRequest,
    hash: &impl DigestPort,
) -> Result<DecodedPreparation, PreparationError> {
    if request.payload.starts_with(b"HFCQ\x01") {
        crate::claude_qualification::decode(request, hash)
            .map(|v| DecodedPreparation::ClaudeQualification(Box::new(v)))
    } else if request.payload.starts_with(b"HFPL\x01") {
        decode(request, hash).map(|v| DecodedPreparation::FixedProtocol(Box::new(v)))
    } else {
        owner_preparation::decode(request, hash).map(|v| DecodedPreparation::Legacy(Box::new(v)))
    }
}
pub fn validate(
    request: &PreparedRequest,
    p: &StorageParticipant,
    hash: &impl DigestPort,
) -> Result<DecodedPreparation, PreparationError> {
    let value = decode_preparation(request, hash)?;
    let policy = value.policy();
    if p.operation != request.operation
        || p.attempt != request.attempt
        || p.source.generation != request.generation
        || p.request_digest != request.digest
        || p.configuration_digest != policy.configuration_digest
        || p.profile_digest != policy.profile_digest
    {
        return Err(PreparationError::Binding);
    }
    if let DecodedPreparation::FixedProtocol(v) = &value
        && v.source != p.source
    {
        return Err(PreparationError::Binding);
    }
    if let DecodedPreparation::ClaudeQualification(v) = &value
        && v.source != p.source
    {
        return Err(PreparationError::Binding);
    }
    Ok(value)
}
fn check(v: &PreparedLaunch) -> Result<(), PreparationError> {
    let m = &v.binary.metadata;
    if v.assignment != LaunchAssignment::FixedProtocolHelper
        || v.policy.assignment != FixtureAssignment::FixedEffect
        || v.argv != [FIXED_PROTOCOL_ARG]
        || !v.environment.is_empty()
        || v.source_profile != SOURCE_PROFILE
        || v.source.generation != v.policy.generation
        || m.length == 0
        || m.length > 128 * 1024 * 1024
        || m.identity.inode == 0
        || !(0..1_000_000_000).contains(&m.modified_nanoseconds)
        || !(0..1_000_000_000).contains(&m.changed_nanoseconds)
        || v.ledger.inode == 0
    {
        return Err(PreparationError::Binding);
    }
    for path in [&v.binary.logical_path, &v.binary.physical_path, &v.cwd] {
        if !crate::fixture::canonical_path(path) {
            return Err(PreparationError::Shape);
        }
    }
    for id in [
        &v.source.root,
        &v.source.database,
        &v.source.marker,
        &v.source.store_lock,
        &v.source.io_lock,
    ] {
        if id.inode == 0 {
            return Err(PreparationError::Binding);
        }
    }
    Ok(())
}

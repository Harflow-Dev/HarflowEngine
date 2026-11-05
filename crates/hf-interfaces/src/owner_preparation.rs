//! Closed private mechanics preparation, not a public execution or permission grant.
use hf_application::owner_runtime::DigestPort;
use hf_domain::{owner_policy::*, owner_runtime::StorageParticipant, ownership::*};
mod read;
pub const PROFILE_DIGEST: &str = "7df0c93a17b185620e35fcf0d0a3e060174ff0445318ed505dc7e4573a08c6d6";
pub const MAX_PAYLOAD: usize = 16384;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationError {
    Shape,
    Binding,
    Policy,
}

pub fn encode(value: &PreparationEnvelope) -> Result<Vec<u8>, PreparationError> {
    check(value)?;
    let mut out = b"HFOP\x01".to_vec();
    for s in [
        value.profile_digest.as_str(),
        value.configuration_digest.as_str(),
        value.operation.as_str(),
        value.session.as_str(),
        value.reservation.as_str(),
        value.attempt.as_str(),
        value.generation.as_str(),
    ] {
        out.extend_from_slice(s.as_bytes());
    }
    out.push(match value.policy.background_requirement {
        Requirement::Required => 0,
        Requirement::Incidental => 1,
        Requirement::Unknown => return Err(PreparationError::Policy),
    });
    out.push(match value.policy.permission_mode {
        PermissionMode::OsEnforced => 0,
        PermissionMode::ToolLevel => 1,
    });
    out.extend_from_slice(&value.policy.background_idle_seconds.to_bits().to_be_bytes());
    out.push(u8::from(value.policy.tool_level_opt_in));
    let a = &value.authority;
    out.extend_from_slice(&a.caller.pid().to_be_bytes());
    out.extend_from_slice(&a.caller.start_seconds().to_be_bytes());
    out.extend_from_slice(&a.caller.start_microseconds().to_be_bytes());
    out.extend_from_slice(a.caller.boot_uuid().as_bytes());
    out.push(match a.role {
        AuthorityRole::Direct => 0,
        AuthorityRole::Worker => 1,
        AuthorityRole::Builder => 2,
        AuthorityRole::Orchestrator => 3,
    });
    match (&a.checkpoint, a.revision) {
        (None, None) => out.push(0),
        (Some(id), Some(revision)) => {
            out.push(1);
            out.extend_from_slice(id.as_str().as_bytes());
            out.extend_from_slice(&revision.to_be_bytes());
        }
        _ => return Err(PreparationError::Binding),
    }
    out.extend_from_slice(a.issued_attempt.as_str().as_bytes());
    out.extend_from_slice(a.grant_digest.as_str().as_bytes());
    out.push(match value.assignment {
        FixtureAssignment::FixedEffect => 0,
        FixtureAssignment::SaturatedEffect => 1,
    });
    Ok(out)
}

pub fn decode(
    request: &PreparedRequest,
    hash: &impl DigestPort,
) -> Result<PreparationEnvelope, PreparationError> {
    if request.payload.len() > MAX_PAYLOAD
        || hash.digest_parts(&[&request.payload]) != request.digest
    {
        return Err(PreparationError::Binding);
    }
    let value = read::decode(&request.payload)?;
    check(&value)?;
    if value.operation != request.operation
        || value.session != request.session
        || value.reservation != request.reservation
        || value.attempt != request.attempt
        || value.generation != request.generation
    {
        return Err(PreparationError::Binding);
    }
    Ok(value)
}

pub fn validate(
    request: &PreparedRequest,
    participant: &StorageParticipant,
    hash: &impl DigestPort,
) -> Result<PreparationEnvelope, PreparationError> {
    let value = decode(request, hash)?;
    if participant.operation != request.operation
        || participant.attempt != request.attempt
        || participant.source.generation != request.generation
        || participant.request_digest != request.digest
        || participant.configuration_digest != value.configuration_digest
        || participant.profile_digest != value.profile_digest
    {
        return Err(PreparationError::Binding);
    }
    Ok(value)
}

fn check(value: &PreparationEnvelope) -> Result<(), PreparationError> {
    let p = &value.policy;
    if value.profile_digest.as_str() != PROFILE_DIGEST
        || !p.background_idle_seconds.is_finite()
        || p.background_idle_seconds <= 0.0
        || p.background_idle_seconds > 21600.0
        || p.background_requirement == Requirement::Unknown
        || p.tool_level_opt_in != (p.permission_mode == PermissionMode::ToolLevel)
    {
        return Err(PreparationError::Policy);
    }
    let a = &value.authority;
    if a.issued_attempt != value.attempt
        || match a.role {
            AuthorityRole::Direct => a.checkpoint.is_some() || a.revision.is_some(),
            _ => a.checkpoint.is_none() || a.revision.is_none(),
        }
    {
        return Err(PreparationError::Binding);
    }
    Ok(())
}

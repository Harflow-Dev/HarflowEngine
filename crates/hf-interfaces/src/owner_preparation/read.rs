use super::PreparationError;
use hf_domain::{owner_policy::*, ownership::*, process::ProcessIdentity};
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], PreparationError> {
        let end = self.at.checked_add(N).ok_or(PreparationError::Shape)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(PreparationError::Shape)?;
        self.at = end;
        value.try_into().map_err(|_| PreparationError::Shape)
    }
    fn text<const N: usize>(&mut self) -> Result<String, PreparationError> {
        let bytes = self.take::<N>()?;
        if !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        {
            return Err(PreparationError::Shape);
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| PreparationError::Shape)
    }
    fn id(&mut self) -> Result<Id, PreparationError> {
        Id::new(self.text::<32>()?).map_err(|_| PreparationError::Shape)
    }
    fn digest(&mut self) -> Result<Digest, PreparationError> {
        Digest::new(self.text::<64>()?).map_err(|_| PreparationError::Shape)
    }
    fn byte(&mut self) -> Result<u8, PreparationError> {
        Ok(self.take::<1>()?[0])
    }
    fn u64(&mut self) -> Result<u64, PreparationError> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn boolean(&mut self) -> Result<bool, PreparationError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(PreparationError::Shape),
        }
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<PreparationEnvelope, PreparationError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take::<5>()? != *b"HFOP\x01" {
        return Err(PreparationError::Shape);
    }
    let profile_digest = r.digest()?;
    let configuration_digest = r.digest()?;
    let operation = r.id()?;
    let session = r.id()?;
    let reservation = r.id()?;
    let attempt = r.id()?;
    let generation = r.id()?;
    let background_requirement = match r.byte()? {
        0 => Requirement::Required,
        1 => Requirement::Incidental,
        _ => return Err(PreparationError::Policy),
    };
    let permission_mode = match r.byte()? {
        0 => PermissionMode::OsEnforced,
        1 => PermissionMode::ToolLevel,
        _ => return Err(PreparationError::Policy),
    };
    let background_idle_seconds = f64::from_bits(r.u64()?);
    let tool_level_opt_in = r.boolean()?;
    let pid = u32::from_be_bytes(r.take()?);
    let seconds = r.u64()?;
    let micros = r.u64()?;
    let caller = ProcessIdentity::new(pid, seconds, micros, r.text::<36>()?)
        .map_err(|_| PreparationError::Shape)?;
    let role = match r.byte()? {
        0 => AuthorityRole::Direct,
        1 => AuthorityRole::Worker,
        2 => AuthorityRole::Builder,
        3 => AuthorityRole::Orchestrator,
        _ => return Err(PreparationError::Shape),
    };
    let (checkpoint, revision) = match r.byte()? {
        0 => (None, None),
        1 => (Some(r.id()?), Some(r.u64()?)),
        _ => return Err(PreparationError::Shape),
    };
    let issued_attempt = r.id()?;
    let grant_digest = r.digest()?;
    let assignment = match r.byte()? {
        0 => FixtureAssignment::FixedEffect,
        1 => FixtureAssignment::SaturatedEffect,
        _ => return Err(PreparationError::Shape),
    };
    if r.at != bytes.len() {
        return Err(PreparationError::Shape);
    }
    Ok(PreparationEnvelope {
        profile_digest,
        configuration_digest,
        operation,
        session,
        reservation,
        attempt,
        generation,
        policy: RuntimePolicy {
            background_requirement,
            permission_mode,
            background_idle_seconds,
            tool_level_opt_in,
        },
        authority: AuthorityContext {
            caller,
            role,
            checkpoint,
            revision,
            issued_attempt,
            grant_digest,
        },
        assignment,
    })
}

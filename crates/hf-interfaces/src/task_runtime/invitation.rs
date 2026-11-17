//! Strict production HFTI-v2 invitation. Never promotes HFOP fixture assignments.
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    capture::FileIdentity,
    ownership::{Digest, Id},
    prepared_launch::{BinaryMetadata, BinaryPin},
    process::ProcessIdentity,
    recovery::SourceStamp,
    task_launch::CliSessionId,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::ProductionInvitation,
};
const POLICY: &str = "first-public-task-v1/unrestricted/incidental";
const BUDGET: &str = "first-public-task-budget-v1";
pub fn encode_invitation(v: &ProductionInvitation) -> Result<Vec<u8>, InvalidIntent> {
    if v.source.generation != v.binding.generation
        || !v.cwd.starts_with('/')
        || v.expected_version.is_empty()
    {
        return Err(InvalidIntent);
    }
    let mut out = b"HFTI\x02".to_vec();
    for value in [
        v.binding.task.as_str(),
        v.binding.operation.as_str(),
        v.binding.attempt.as_str(),
        v.binding.generation.as_str(),
        v.reservation.as_str(),
        v.ownership_session.as_str(),
        v.session.as_str(),
        POLICY,
        BUDGET,
    ] {
        text(&mut out, value)?;
    }
    for i in [
        &v.source.root,
        &v.source.database,
        &v.source.marker,
        &v.source.store_lock,
        &v.source.io_lock,
    ] {
        number(&mut out, i.device);
        number(&mut out, i.inode);
    }
    for process in [&v.caller, &v.dispatcher] {
        number(&mut out, u64::from(process.pid()));
        number(&mut out, process.start_seconds());
        number(&mut out, process.start_microseconds());
        text(&mut out, process.boot_uuid())?;
    }
    for pin in [&v.ctl, &v.backend] {
        pin_encode(&mut out, pin)?;
    }
    text(&mut out, &v.cwd)?;
    for digest in [
        &v.environment_digest,
        &v.prompt_digest,
        &v.metadata_digest,
        &v.configuration_digest,
        &v.invocation_digest,
    ] {
        text(&mut out, digest.as_str())?;
    }
    text(&mut out, &v.expected_version)?;
    if out.len() > 16384 {
        return Err(InvalidIntent);
    }
    Ok(out)
}
pub(super) fn number(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes());
}
pub(super) fn text(out: &mut Vec<u8>, v: &str) -> Result<(), InvalidIntent> {
    if v.len() > 4096 || v.contains('\0') {
        return Err(InvalidIntent);
    }
    out.extend_from_slice(&(v.len() as u32).to_be_bytes());
    out.extend_from_slice(v.as_bytes());
    Ok(())
}
fn pin_encode(out: &mut Vec<u8>, pin: &BinaryPin) -> Result<(), InvalidIntent> {
    if !pin.logical_path.starts_with('/')
        || !pin.physical_path.starts_with('/')
        || !(0..1_000_000_000).contains(&pin.metadata.modified_nanoseconds)
        || !(0..1_000_000_000).contains(&pin.metadata.changed_nanoseconds)
    {
        return Err(InvalidIntent);
    }
    text(out, &pin.logical_path)?;
    text(out, &pin.physical_path)?;
    let m = &pin.metadata;
    for n in [
        m.identity.device,
        m.identity.inode,
        m.length,
        m.modified_seconds as u64,
        m.modified_nanoseconds as u64,
        m.changed_seconds as u64,
        m.changed_nanoseconds as u64,
    ] {
        number(out, n);
    }
    text(out, pin.digest.as_str())
}
pub(super) struct Reader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) at: usize,
}
impl Reader<'_> {
    pub(super) fn take(&mut self, n: usize) -> Result<&[u8], InvalidIntent> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(InvalidIntent)?;
        let result = &self.bytes[self.at..end];
        self.at = end;
        Ok(result)
    }
    pub(super) fn number(&mut self) -> Result<u64, InvalidIntent> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| InvalidIntent)?,
        ))
    }
    pub(super) fn text(&mut self) -> Result<String, InvalidIntent> {
        let len = u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| InvalidIntent)?) as usize;
        if len > 4096 {
            return Err(InvalidIntent);
        }
        let result = std::str::from_utf8(self.take(len)?)
            .map_err(|_| InvalidIntent)?
            .to_owned();
        if result.contains('\0') {
            return Err(InvalidIntent);
        }
        Ok(result)
    }
    pub(super) fn id(&mut self) -> Result<Id, InvalidIntent> {
        let s = self.text()?;
        let id = Id::new(s.clone()).map_err(|_| InvalidIntent)?;
        if id.as_str() != s {
            return Err(InvalidIntent);
        }
        Ok(id)
    }
    fn digest(&mut self) -> Result<Digest, InvalidIntent> {
        let s = self.text()?;
        let id = Digest::new(s.clone()).map_err(|_| InvalidIntent)?;
        if id.as_str() != s {
            return Err(InvalidIntent);
        }
        Ok(id)
    }
    fn file(&mut self) -> Result<FileIdentity, InvalidIntent> {
        Ok(FileIdentity {
            device: self.number()?,
            inode: self.number()?,
        })
    }
    pub(super) fn process(&mut self) -> Result<ProcessIdentity, InvalidIntent> {
        ProcessIdentity::new(
            u32::try_from(self.number()?).map_err(|_| InvalidIntent)?,
            self.number()?,
            self.number()?,
            self.text()?,
        )
        .map_err(|_| InvalidIntent)
    }
    fn pin(&mut self) -> Result<BinaryPin, InvalidIntent> {
        Ok(BinaryPin {
            logical_path: self.text()?,
            physical_path: self.text()?,
            metadata: BinaryMetadata {
                identity: self.file()?,
                length: self.number()?,
                modified_seconds: self.number()? as i64,
                modified_nanoseconds: self.number()? as i64,
                changed_seconds: self.number()? as i64,
                changed_nanoseconds: self.number()? as i64,
            },
            digest: self.digest()?,
        })
    }
}
pub fn decode_invitation(bytes: &[u8]) -> Result<ProductionInvitation, InvalidIntent> {
    if bytes.len() > 16384 {
        return Err(InvalidIntent);
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTI\x02" {
        return Err(InvalidIntent);
    }
    let binding = LifecycleBinding {
        task: TaskId::new(r.text()?).map_err(|_| InvalidIntent)?,
        operation: r.id()?,
        attempt: r.id()?,
        generation: r.id()?,
    };
    let reservation = r.id()?;
    let ownership_session = r.id()?;
    let session = CliSessionId::new(r.text()?).map_err(|_| InvalidIntent)?;
    if r.text() != Ok(POLICY.into()) || r.text() != Ok(BUDGET.into()) {
        return Err(InvalidIntent);
    }
    let source = SourceStamp {
        generation: binding.generation.clone(),
        root: r.file()?,
        database: r.file()?,
        marker: r.file()?,
        store_lock: r.file()?,
        io_lock: r.file()?,
    };
    let value = ProductionInvitation {
        binding,
        reservation,
        ownership_session,
        session,
        source,
        caller: r.process()?,
        dispatcher: r.process()?,
        ctl: r.pin()?,
        backend: r.pin()?,
        cwd: r.text()?,
        environment_digest: r.digest()?,
        prompt_digest: r.digest()?,
        metadata_digest: r.digest()?,
        configuration_digest: r.digest()?,
        invocation_digest: r.digest()?,
        expected_version: r.text()?,
    };
    if r.at != bytes.len() || encode_invitation(&value)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(value)
}

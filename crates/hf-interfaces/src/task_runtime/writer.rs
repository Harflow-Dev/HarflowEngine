//! Production-only writer handoff. Live parent/self, retained root and binary validation
//! are mandatory host/platform checks; possession of these bytes grants nothing.
use super::invitation::{Reader, decode_invitation, encode_invitation, number, text};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    ownership::OwnerClaim,
    process::ProcessIdentity,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{WriterBootstrap, WriterReady},
};
pub const WRITER_PAYLOAD_BYTES: usize = 16380;
fn process(out: &mut Vec<u8>, p: &ProcessIdentity) -> Result<(), InvalidIntent> {
    number(out, u64::from(p.pid()));
    number(out, p.start_seconds());
    number(out, p.start_microseconds());
    text(out, p.boot_uuid())
}
fn owner(out: &mut Vec<u8>, p: &OwnerClaim) -> Result<(), InvalidIntent> {
    text(out, p.owner.as_str())?;
    text(out, p.nonce.as_str())?;
    process(out, &p.process)
}
fn read_owner(r: &mut Reader<'_>) -> Result<OwnerClaim, InvalidIntent> {
    Ok(OwnerClaim {
        owner: r.id()?,
        nonce: r.id()?,
        process: r.process()?,
    })
}
fn root_valid(root: &str) -> bool {
    root.starts_with('/')
        && root.len() <= 4096
        && !root.chars().any(char::is_control)
        && !root
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
}
pub fn encode_writer_bootstrap(v: &WriterBootstrap) -> Result<Vec<u8>, InvalidIntent> {
    if !root_valid(&v.root)
        || v.owner.process.pid() == v.writer.pid()
        || v.owner.process.boot_uuid() != v.writer.boot_uuid()
        || v.owner.process.boot_uuid() != v.invitation.dispatcher.boot_uuid()
    {
        return Err(InvalidIntent);
    }
    let invitation = encode_invitation(&v.invitation)?;
    let mut out = b"HFTW\x01".to_vec();
    text(&mut out, &v.root)?;
    number(&mut out, invitation.len() as u64);
    out.extend_from_slice(&invitation);
    owner(&mut out, &v.owner)?;
    process(&mut out, &v.writer)?;
    if out.len() > WRITER_PAYLOAD_BYTES {
        return Err(InvalidIntent);
    }
    Ok(out)
}
pub fn decode_writer_bootstrap(bytes: &[u8]) -> Result<WriterBootstrap, InvalidIntent> {
    if bytes.len() > WRITER_PAYLOAD_BYTES {
        return Err(InvalidIntent);
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTW\x01" {
        return Err(InvalidIntent);
    }
    let root = r.text()?;
    let len = usize::try_from(r.number()?).map_err(|_| InvalidIntent)?;
    if len > 16384 {
        return Err(InvalidIntent);
    }
    let invitation = decode_invitation(r.take(len)?)?;
    let owner = read_owner(&mut r)?;
    let writer = r.process()?;
    let value = WriterBootstrap {
        root,
        invitation,
        owner,
        writer,
    };
    if r.at != bytes.len() || encode_writer_bootstrap(&value)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(value)
}
pub fn encode_writer_ready(v: &WriterReady) -> Result<Vec<u8>, InvalidIntent> {
    if v.revision != 1
        || v.ownership_revision != 1
        || v.owner.process.pid() == v.writer.pid()
        || v.owner.process.boot_uuid() != v.writer.boot_uuid()
    {
        return Err(InvalidIntent);
    }
    let mut out = b"HFTR\x01".to_vec();
    for value in [
        v.binding.task.as_str(),
        v.binding.operation.as_str(),
        v.binding.attempt.as_str(),
        v.binding.generation.as_str(),
    ] {
        text(&mut out, value)?;
    }
    process(&mut out, &v.writer)?;
    owner(&mut out, &v.owner)?;
    number(&mut out, v.revision);
    number(&mut out, v.ownership_revision);
    if out.len() > WRITER_PAYLOAD_BYTES {
        return Err(InvalidIntent);
    }
    Ok(out)
}
pub fn decode_writer_ready(bytes: &[u8]) -> Result<WriterReady, InvalidIntent> {
    if bytes.len() > WRITER_PAYLOAD_BYTES {
        return Err(InvalidIntent);
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTR\x01" {
        return Err(InvalidIntent);
    }
    let binding = LifecycleBinding {
        task: TaskId::new(r.text()?).map_err(|_| InvalidIntent)?,
        operation: r.id()?,
        attempt: r.id()?,
        generation: r.id()?,
    };
    let value = WriterReady {
        binding,
        writer: r.process()?,
        owner: read_owner(&mut r)?,
        revision: r.number()?,
        ownership_revision: r.number()?,
    };
    if r.at != bytes.len() || encode_writer_ready(&value)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(value)
}

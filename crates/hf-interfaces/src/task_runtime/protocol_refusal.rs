//! The positive pre-agent refusal (`NotStarted`) codec and its probe attestation.
use super::{
    invitation::{Reader, number, text},
    protocol::{envelope, finish, process, read_envelope},
};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{process::ProcessExit, task_runtime::*};

pub(super) fn probe(out: &mut Vec<u8>, v: &ProbeAttestation) -> Result<(), InvalidIntent> {
    if v.exit != ProcessExit::Code(0) || v.stdout_bytes > 4096 || v.stderr_bytes != 0 {
        return Err(InvalidIntent);
    }
    process(out, &v.child)?;
    text(out, v.backend_digest.as_str())?;
    number(out, v.stdout_bytes);
    number(out, v.stderr_bytes);
    Ok(())
}
pub(super) fn read_probe(r: &mut Reader<'_>) -> Result<ProbeAttestation, InvalidIntent> {
    Ok(ProbeAttestation {
        child: r.process()?,
        exit: ProcessExit::Code(0),
        backend_digest: hf_domain::ownership::Digest::new(r.text()?).map_err(|_| InvalidIntent)?,
        stdout_bytes: r.number()?,
        stderr_bytes: r.number()?,
    })
}
pub fn encode_not_started(v: &NotStarted) -> Result<Vec<u8>, InvalidIntent> {
    if v.reason != "unsupported_version" {
        return Err(InvalidIntent);
    }
    let mut out = b"HFTN\x01".to_vec();
    envelope(&mut out, &v.envelope)?;
    text(&mut out, &v.reason)?;
    probe(&mut out, &v.proof)?;
    finish(out)
}
pub fn decode_not_started(bytes: &[u8]) -> Result<NotStarted, InvalidIntent> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTN\x01" {
        return Err(InvalidIntent);
    }
    let v = NotStarted {
        envelope: read_envelope(&mut r)?,
        reason: r.text()?,
        proof: read_probe(&mut r)?,
    };
    if r.at != bytes.len() || encode_not_started(&v)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(v)
}

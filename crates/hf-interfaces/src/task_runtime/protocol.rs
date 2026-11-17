//! Strict, bounded private command and positive acknowledgement codecs.
use super::invitation::{Reader, number, text};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    process::{ProcessExit, ProcessIdentity},
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::*,
};
fn process(out: &mut Vec<u8>, p: &ProcessIdentity) -> Result<(), InvalidIntent> {
    number(out, p.pid().into());
    number(out, p.start_seconds());
    number(out, p.start_microseconds());
    text(out, p.boot_uuid())
}
fn envelope(out: &mut Vec<u8>, v: &WriterEnvelope) -> Result<(), InvalidIntent> {
    if v.revision == 0 || v.ownership_revision == 0 || v.owner.process.pid() == v.writer.pid() {
        return Err(InvalidIntent);
    }
    for s in [
        v.binding.task.as_str(),
        v.binding.operation.as_str(),
        v.binding.attempt.as_str(),
        v.binding.generation.as_str(),
        v.owner.owner.as_str(),
        v.owner.nonce.as_str(),
    ] {
        text(out, s)?;
    }
    process(out, &v.owner.process)?;
    process(out, &v.writer)?;
    number(out, v.revision);
    number(out, v.ownership_revision);
    Ok(())
}
fn read_envelope(r: &mut Reader<'_>) -> Result<WriterEnvelope, InvalidIntent> {
    let binding = LifecycleBinding {
        task: TaskId::new(r.text()?).map_err(|_| InvalidIntent)?,
        operation: r.id()?,
        attempt: r.id()?,
        generation: r.id()?,
    };
    let owner = hf_domain::ownership::OwnerClaim {
        owner: r.id()?,
        nonce: r.id()?,
        process: r.process()?,
    };
    Ok(WriterEnvelope {
        binding,
        owner,
        writer: r.process()?,
        revision: r.number()?,
        ownership_revision: r.number()?,
    })
}
fn boolean(r: &mut Reader<'_>) -> Result<bool, InvalidIntent> {
    match r.number()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(InvalidIntent),
    }
}
fn finish(out: Vec<u8>) -> Result<Vec<u8>, InvalidIntent> {
    if out.len() > 24576 {
        Err(InvalidIntent)
    } else {
        Ok(out)
    }
}
pub fn encode_writer_command(v: &WriterCommand) -> Result<Vec<u8>, InvalidIntent> {
    let mut out = b"HFTQ\x01".to_vec();
    envelope(&mut out, &v.envelope)?;
    match &v.operation {
        WriterOperation::NotStarted { proof, reason } => {
            number(&mut out, 3);
            probe(&mut out, proof)?;
            text(&mut out, reason)?;
        }
        WriterOperation::Spawn { child, started_at } => {
            number(&mut out, 0);
            process(&mut out, child)?;
            text(&mut out, started_at)?;
        }
        WriterOperation::Capture {
            stderr,
            offset,
            bytes,
        } => {
            if bytes.is_empty() || bytes.len() > 16384 {
                return Err(InvalidIntent);
            }
            number(&mut out, 1);
            number(&mut out, u64::from(*stderr));
            number(&mut out, *offset);
            number(&mut out, bytes.len() as u64);
            out.extend_from_slice(bytes);
        }
        WriterOperation::Finish { proof, finished_at } => {
            number(&mut out, 2);
            process(&mut out, &proof.child)?;
            let (kind, code) = match &proof.exit {
                ProcessExit::Code(n) => (0, *n),
                ProcessExit::Signal(n) => (1, *n),
                ProcessExit::Unobserved => return Err(InvalidIntent),
            };
            number(&mut out, kind);
            number(&mut out, code as u64);
            number(&mut out, proof.stdout_end);
            number(&mut out, proof.stderr_end);
            number(&mut out, proof.input_bytes);
            text(&mut out, proof.source_binding.as_str())?;
            text(&mut out, finished_at)?;
        }
    }
    finish(out)
}
pub fn decode_writer_command(bytes: &[u8]) -> Result<WriterCommand, InvalidIntent> {
    if bytes.len() > 24576 {
        return Err(InvalidIntent);
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTQ\x01" {
        return Err(InvalidIntent);
    }
    let envelope = read_envelope(&mut r)?;
    let operation = match r.number()? {
        3 => WriterOperation::NotStarted {
            proof: read_probe(&mut r)?,
            reason: r.text()?,
        },
        0 => WriterOperation::Spawn {
            child: r.process()?,
            started_at: r.text()?,
        },
        1 => {
            let stderr = boolean(&mut r)?;
            let offset = r.number()?;
            let n = usize::try_from(r.number()?).map_err(|_| InvalidIntent)?;
            if n > 16384 {
                return Err(InvalidIntent);
            }
            WriterOperation::Capture {
                stderr,
                offset,
                bytes: r.take(n)?.to_vec(),
            }
        }
        2 => {
            let child = r.process()?;
            let kind = r.number()?;
            let code = i32::try_from(r.number()? as i64).map_err(|_| InvalidIntent)?;
            let exit = match kind {
                0 => ProcessExit::Code(code),
                1 if code > 0 => ProcessExit::Signal(code),
                _ => return Err(InvalidIntent),
            };
            WriterOperation::Finish {
                proof: SettlementAttestation {
                    child,
                    exit,
                    stdout_end: r.number()?,
                    stderr_end: r.number()?,
                    input_bytes: r.number()?,
                    source_binding: hf_domain::ownership::Digest::new(r.text()?)
                        .map_err(|_| InvalidIntent)?,
                },
                finished_at: r.text()?,
            }
        }
        _ => return Err(InvalidIntent),
    };
    let v = WriterCommand {
        envelope,
        operation,
    };
    if r.at != bytes.len() || encode_writer_command(&v)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(v)
}
pub fn encode_writer_ack(v: &WriterAcknowledgement) -> Result<Vec<u8>, InvalidIntent> {
    let mut out = b"HFTA\x01".to_vec();
    envelope(&mut out, &v.envelope)?;
    number(&mut out, v.stdout_end);
    number(&mut out, v.stderr_end);
    number(&mut out, u64::from(v.close_input));
    number(&mut out, u64::from(v.terminal));
    text(
        &mut out,
        v.source_binding.as_ref().map_or("", |v| v.as_str()),
    )?;
    finish(out)
}
pub fn decode_writer_ack(bytes: &[u8]) -> Result<WriterAcknowledgement, InvalidIntent> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTA\x01" {
        return Err(InvalidIntent);
    }
    let v = WriterAcknowledgement {
        envelope: read_envelope(&mut r)?,
        stdout_end: r.number()?,
        stderr_end: r.number()?,
        close_input: boolean(&mut r)?,
        terminal: boolean(&mut r)?,
        source_binding: {
            let text = r.text()?;
            if text.is_empty() {
                None
            } else {
                Some(hf_domain::ownership::Digest::new(text).map_err(|_| InvalidIntent)?)
            }
        },
    };
    if r.at != bytes.len() || encode_writer_ack(&v)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(v)
}
pub fn encode_started(v: &Started) -> Result<Vec<u8>, InvalidIntent> {
    let mut out = b"HFTS\x01".to_vec();
    envelope(&mut out, &v.envelope)?;
    process(&mut out, &v.child)?;
    text(&mut out, v.source_binding.as_str())?;
    finish(out)
}
pub fn decode_started(bytes: &[u8]) -> Result<Started, InvalidIntent> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTS\x01" {
        return Err(InvalidIntent);
    }
    let v = Started {
        envelope: read_envelope(&mut r)?,
        child: r.process()?,
        source_binding: hf_domain::ownership::Digest::new(r.text()?).map_err(|_| InvalidIntent)?,
    };
    if r.at != bytes.len() || encode_started(&v)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(v)
}

fn probe(out: &mut Vec<u8>, v: &ProbeAttestation) -> Result<(), InvalidIntent> {
    if v.exit != ProcessExit::Code(0) || v.stdout_bytes > 4096 || v.stderr_bytes != 0 {
        return Err(InvalidIntent);
    }
    process(out, &v.child)?;
    text(out, v.backend_digest.as_str())?;
    number(out, v.stdout_bytes);
    number(out, v.stderr_bytes);
    Ok(())
}
fn read_probe(r: &mut Reader<'_>) -> Result<ProbeAttestation, InvalidIntent> {
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

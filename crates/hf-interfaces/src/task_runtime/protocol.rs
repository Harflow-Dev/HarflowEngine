//! Strict, bounded private command and positive acknowledgement codecs.
use super::{
    invitation::{Reader, number, text},
    protocol_finish::{decode_finish, encode_finish},
    protocol_refusal::{probe, read_probe},
};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    process::ProcessIdentity, task_lifecycle::LifecycleBinding, task_read::TaskId, task_runtime::*,
};
pub(super) fn process(out: &mut Vec<u8>, p: &ProcessIdentity) -> Result<(), InvalidIntent> {
    number(out, p.pid().into());
    number(out, p.start_seconds());
    number(out, p.start_microseconds());
    text(out, p.boot_uuid())
}
pub(super) fn envelope(out: &mut Vec<u8>, v: &WriterEnvelope) -> Result<(), InvalidIntent> {
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
pub(super) fn read_envelope(r: &mut Reader<'_>) -> Result<WriterEnvelope, InvalidIntent> {
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
pub(super) fn finish(out: Vec<u8>) -> Result<Vec<u8>, InvalidIntent> {
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
            encode_finish(&mut out, proof, finished_at)?;
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
        opcode @ (2 | 4 | 5) => decode_finish(&mut r, opcode)?,
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

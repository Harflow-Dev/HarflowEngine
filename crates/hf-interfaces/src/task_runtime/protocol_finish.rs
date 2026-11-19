//! The `Finish` command: opcode 2 settles with no capture loss; opcode 4 carries a stall
//! loss and opcode 5 a disconnect loss.
use super::{
    invitation::{Reader, number, text},
    protocol::process,
};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    process::ProcessExit,
    task_runtime::{CaptureLoss, SettlementAttestation, WriterOperation},
};

pub(super) fn encode_finish(
    out: &mut Vec<u8>,
    proof: &SettlementAttestation,
    finished_at: &str,
) -> Result<(), InvalidIntent> {
    // Opcodes 4 and 5 differ from opcode 2 only in recording the cause of a capture loss.
    number(
        out,
        match proof.capture_loss {
            CaptureLoss::None => 2,
            CaptureLoss::Stall => 4,
            CaptureLoss::Disconnect => 5,
        },
    );
    process(out, &proof.child)?;
    let (kind, code) = match &proof.exit {
        ProcessExit::Code(n) => (0, *n),
        ProcessExit::Signal(n) => (1, *n),
        ProcessExit::Unobserved => return Err(InvalidIntent),
    };
    number(out, kind);
    number(out, code as u64);
    number(out, proof.stdout_end);
    number(out, proof.stderr_end);
    number(out, proof.input_bytes);
    text(out, proof.source_binding.as_str())?;
    text(out, finished_at)
}
pub(super) fn decode_finish(
    r: &mut Reader<'_>,
    opcode: u64,
) -> Result<WriterOperation, InvalidIntent> {
    let capture_loss = match opcode {
        2 => CaptureLoss::None,
        4 => CaptureLoss::Stall,
        5 => CaptureLoss::Disconnect,
        _ => return Err(InvalidIntent),
    };
    let child = r.process()?;
    let kind = r.number()?;
    let code = i32::try_from(r.number()? as i64).map_err(|_| InvalidIntent)?;
    let exit = match kind {
        0 => ProcessExit::Code(code),
        1 if code > 0 => ProcessExit::Signal(code),
        _ => return Err(InvalidIntent),
    };
    Ok(WriterOperation::Finish {
        proof: SettlementAttestation {
            child,
            exit,
            stdout_end: r.number()?,
            stderr_end: r.number()?,
            input_bytes: r.number()?,
            source_binding: hf_domain::ownership::Digest::new(r.text()?)
                .map_err(|_| InvalidIntent)?,
            capture_loss,
        },
        finished_at: r.text()?,
    })
}

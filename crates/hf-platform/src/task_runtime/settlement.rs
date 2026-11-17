//! Original-handle settlement capability. Decoded transport facts cannot construct it.
use crate::process::{CaptureOrigin, CaptureReader, RetainedOwnedChild};
use hf_domain::{
    process::ProcessExit,
    task_runtime::{SettlementAttestation, WriterOperation},
};
use std::{io, sync::mpsc::TryRecvError};
pub struct OwnerSettlement {
    proof: SettlementAttestation,
}
impl OwnerSettlement {
    pub fn qualify(
        child: &RetainedOwnedChild,
        reader: &CaptureReader,
        origin: &CaptureOrigin,
        progress: (u64, u64, u64),
        source_binding: hf_domain::ownership::Digest,
    ) -> io::Result<Self> {
        let (out, err, expected_input) = progress;
        let fail = || io::Error::other("original owner settlement refused");
        let (identity, exit, input_bytes) = child.settlement_state().ok_or_else(fail)?;
        let final_value = reader.joined_final().ok_or_else(fail)?;
        let p = &final_value.progress;
        if identity != origin.child
            || reader.origin() != origin
            || exit == ProcessExit::Unobserved
            || input_bytes != expected_input
            || input_bytes == 0
            || final_value.stopped
            || final_value.limited
            || p.discard
            || !p.stdout.eof
            || !p.stderr.eof
            || p.stdout.error.is_some()
            || p.stderr.error.is_some()
            || p.stdout.discarded != 0
            || p.stderr.discarded != 0
            || p.stdout.read != out
            || p.stdout.enqueued != out
            || p.stderr.read != err
            || p.stderr.enqueued != err
            || !matches!(
                reader.try_recv(),
                Err(TryRecvError::Empty | TryRecvError::Disconnected)
            )
        {
            return Err(fail());
        }
        Ok(Self {
            proof: SettlementAttestation {
                child: identity,
                exit,
                stdout_end: out,
                stderr_end: err,
                input_bytes,
                source_binding,
            },
        })
    }
    pub fn finish_operation(self, finished_at: String) -> WriterOperation {
        WriterOperation::Finish {
            proof: self.proof,
            finished_at,
        }
    }
}

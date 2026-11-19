//! Original-handle settlement capability. Decoded transport facts cannot construct it.
use crate::process::{
    CaptureOrigin, CaptureReader, LossCause, ReaderStreamStats, RetainedOwnedChild,
};
use hf_domain::{
    process::ProcessExit,
    task_runtime::{CaptureLoss, SettlementAttestation, WriterOperation},
};
use std::{io, sync::mpsc::TryRecvError};
pub struct OwnerSettlement {
    proof: SettlementAttestation,
}
impl OwnerSettlement {
    /// A settlement with no capture loss: every byte read was delivered.
    pub fn qualify(
        child: &RetainedOwnedChild,
        reader: &CaptureReader,
        origin: &CaptureOrigin,
        progress: (u64, u64, u64),
        source_binding: hf_domain::ownership::Digest,
    ) -> io::Result<Self> {
        Self::admit(child, reader, origin, progress, source_binding, false)
    }
    /// A settlement after the reader's queue stalled or its consumer disconnected. Every
    /// `qualify` constraint holds except that discarded bytes are allowed and accounted: per
    /// stream, bytes read equal bytes delivered plus bytes discarded, and the committed
    /// ends equal those delivered. The attestation carries the reader's cause. A requested
    /// discard never qualifies.
    pub fn qualify_with_loss(
        child: &RetainedOwnedChild,
        reader: &CaptureReader,
        origin: &CaptureOrigin,
        progress: (u64, u64, u64),
        source_binding: hf_domain::ownership::Digest,
    ) -> io::Result<Self> {
        Self::admit(child, reader, origin, progress, source_binding, true)
    }
    fn admit(
        child: &RetainedOwnedChild,
        reader: &CaptureReader,
        origin: &CaptureOrigin,
        progress: (u64, u64, u64),
        source_binding: hf_domain::ownership::Digest,
        loss: bool,
    ) -> io::Result<Self> {
        let (out, err, expected_input) = progress;
        let fail = || io::Error::other("original owner settlement refused");
        let (identity, exit, input_bytes) = child.settlement_state().ok_or_else(fail)?;
        let final_value = reader.joined_final().ok_or_else(fail)?;
        let p = &final_value.progress;
        let discard_ok = if loss {
            p.overflowed && p.discard && !reader.discard_requested()
        } else {
            !p.discard && !p.overflowed
        };
        let capture_loss = attested_loss(loss, p.overflow_cause).ok_or_else(fail)?;
        let stream_ok = |stats: &ReaderStreamStats, committed: u64| {
            let delivered = if loss {
                stats.enqueued.checked_add(stats.discarded) == Some(stats.read)
            } else {
                stats.discarded == 0 && stats.read == committed
            };
            stats.eof
                && stats.error.is_none()
                && stats.pending == 0
                && stats.enqueued == committed
                && delivered
        };
        if identity != origin.child
            || reader.origin() != origin
            || exit == ProcessExit::Unobserved
            || input_bytes != expected_input
            || input_bytes == 0
            || final_value.stopped
            || final_value.limited
            || !discard_ok
            || !stream_ok(&p.stdout, out)
            || !stream_ok(&p.stderr, err)
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
                capture_loss,
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
/// The cause a settlement attests: none without a loss, otherwise exactly the reader's.
fn attested_loss(loss: bool, cause: Option<LossCause>) -> Option<CaptureLoss> {
    match (loss, cause) {
        (false, _) => Some(CaptureLoss::None),
        (true, Some(LossCause::Stall)) => Some(CaptureLoss::Stall),
        (true, Some(LossCause::Disconnect)) => Some(CaptureLoss::Disconnect),
        (true, None) => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settlement_maps_stall_and_disconnect() {
        assert_eq!(attested_loss(false, None), Some(CaptureLoss::None));
        assert_eq!(
            attested_loss(true, Some(LossCause::Stall)),
            Some(CaptureLoss::Stall)
        );
        assert_eq!(
            attested_loss(true, Some(LossCause::Disconnect)),
            Some(CaptureLoss::Disconnect)
        );
        assert_eq!(attested_loss(true, None), None);
    }
}

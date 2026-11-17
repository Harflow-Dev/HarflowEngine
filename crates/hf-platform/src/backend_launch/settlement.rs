//! A version probe's original waited child and actual pipe settlement remain owned.
use crate::process::RetainedOwnedChild;
use hf_domain::{
    prepared_launch::BinaryPin,
    task_runtime::{ProbeAttestation, WriterOperation},
};
pub struct SettledVersionProbe {
    _child: RetainedOwnedChild,
    proof: ProbeAttestation,
}
impl SettledVersionProbe {
    pub(super) fn qualify(
        pending: &mut Option<RetainedOwnedChild>,
        binary: &BinaryPin,
    ) -> Option<Self> {
        let child = pending.as_ref()?;
        let (identity, exit) = child.settled_probe_identity()?;
        let capture = child.capture();
        let proof = ProbeAttestation {
            child: identity,
            exit,
            backend_digest: binary.digest.clone(),
            stdout_bytes: capture.stdout.bytes.len() as u64,
            stderr_bytes: capture.stderr.bytes.len() as u64,
        };
        Some(Self {
            _child: pending.take()?,
            proof,
        })
    }
    pub fn unsupported_version_operation(&self) -> WriterOperation {
        WriterOperation::NotStarted {
            proof: self.proof.clone(),
            reason: "unsupported_version".into(),
        }
    }
}

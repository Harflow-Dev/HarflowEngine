//! Private commands require the qualified original owner/writer pipes.
use crate::{
    ownership::OwnerClaim,
    process::{ProcessExit, ProcessIdentity},
    task_lifecycle::LifecycleBinding,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterEnvelope {
    pub binding: LifecycleBinding,
    pub owner: OwnerClaim,
    pub writer: ProcessIdentity,
    pub revision: u64,
    pub ownership_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriterOperation {
    NotStarted {
        proof: ProbeAttestation,
        reason: String,
    },
    Spawn {
        child: ProcessIdentity,
        started_at: String,
    },
    Capture {
        stderr: bool,
        offset: u64,
        bytes: Vec<u8>,
    },
    Finish {
        proof: SettlementAttestation,
        finished_at: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeAttestation {
    pub child: ProcessIdentity,
    pub exit: ProcessExit,
    pub backend_digest: crate::ownership::Digest,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotStarted {
    pub envelope: WriterEnvelope,
    pub reason: String,
    pub proof: ProbeAttestation,
}
/// Untrusted decoded facts until admitted on the original qualified owner channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementAttestation {
    pub child: ProcessIdentity,
    pub exit: ProcessExit,
    pub stdout_end: u64,
    pub stderr_end: u64,
    pub input_bytes: u64,
    pub source_binding: crate::ownership::Digest,
    /// The reader's bounded queue overflowed: bytes were read but never delivered.
    pub capture_loss: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterCommand {
    pub envelope: WriterEnvelope,
    pub operation: WriterOperation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterAcknowledgement {
    pub envelope: WriterEnvelope,
    pub stdout_end: u64,
    pub stderr_end: u64,
    pub close_input: bool,
    pub terminal: bool,
    pub source_binding: Option<crate::ownership::Digest>,
}
/// Positive launch acknowledgement, distinct from full-control AcceptanceRecorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub envelope: WriterEnvelope,
    pub child: ProcessIdentity,
    pub source_binding: crate::ownership::Digest,
}

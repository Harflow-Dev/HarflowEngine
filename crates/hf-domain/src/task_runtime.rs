//! Private immutable launch intent. It is not a published task or replay authority.
mod wire;
use crate::{ownership::Digest, task_lifecycle::LifecycleBinding, task_read::TaskId};
pub use wire::{
    CaptureLoss, NotStarted, ProbeAttestation, SettlementAttestation, Started,
    WriterAcknowledgement, WriterCommand, WriterEnvelope, WriterOperation,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchIntentCreate {
    pub task: TaskId,
    pub metadata: Vec<u8>,
    pub metadata_digest: Digest,
    pub prompt: String,
    pub prompt_digest: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchIntent {
    pub binding: LifecycleBinding,
    pub preparation_digest: Digest,
    pub preparation: crate::ownership::PreparedRequest,
    pub create: LaunchIntentCreate,
}
/// Operational readiness, admitted by the retained writer capability before acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptanceReadiness {
    pub child: crate::process::ProcessIdentity,
    pub control_endpoint: String,
    pub source_binding: Digest,
}
/// Production inherited invitation data; bytes alone never grant execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductionInvitation {
    pub binding: LifecycleBinding,
    pub reservation: crate::ownership::Id,
    pub ownership_session: crate::ownership::Id,
    pub session: crate::task_launch::CliSessionId,
    pub source: crate::recovery::SourceStamp,
    pub caller: crate::process::ProcessIdentity,
    pub dispatcher: crate::process::ProcessIdentity,
    pub ctl: crate::prepared_launch::BinaryPin,
    pub backend: crate::prepared_launch::BinaryPin,
    pub cwd: String,
    pub environment_digest: Digest,
    pub prompt_digest: Digest,
    pub metadata_digest: Digest,
    pub configuration_digest: Digest,
    pub invocation_digest: Digest,
    pub expected_version: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlRequest {
    pub binding: LifecycleBinding,
    pub request_id: crate::ownership::Id,
    pub kind: ControlKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Cancel,
}
/// Private production writer bootstrap; decoded fields are not process/root authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterBootstrap {
    pub root: String,
    pub invitation: ProductionInvitation,
    pub owner: crate::ownership::OwnerClaim,
    pub writer: crate::process::ProcessIdentity,
}
/// Sent only after the original writer positively commits OwnerClaimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterReady {
    pub binding: LifecycleBinding,
    pub writer: crate::process::ProcessIdentity,
    pub owner: crate::ownership::OwnerClaim,
    pub revision: u64,
    pub ownership_revision: u64,
}

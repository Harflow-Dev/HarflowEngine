use super::{
    AckState, AuthorityRole, CaptureState, ClaimsState, ContainmentState, DeadlineState,
    ExecutionState, ExitState, FixtureAssignment, InputState, InventoryState, MatchState,
    ObligationState, PermissionMode, PipeState, QualificationState, ReleaseEvidence, Requirement,
    ResultBinding, ResultState, SendState, StorageModelCommand, WatermarkState, WorkflowState,
    WriterState,
};
use crate::ownership::{Digest, Id};
use crate::process::ProcessIdentity;

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimePolicy {
    pub background_requirement: Requirement,
    pub permission_mode: PermissionMode,
    pub background_idle_seconds: f64,
    pub tool_level_opt_in: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionObservation {
    pub requested_mode: PermissionMode,
    pub established_mode: Option<PermissionMode>,
    pub tool_level_opt_in: bool,
    pub qualification: QualificationState,
    pub matches: [MatchState; 4],
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeObservation {
    pub agent_result: ResultState,
    pub result_binding: ResultBinding,
    pub sticky_failure: bool,
    pub turn_open: bool,
    pub requirement: Requirement,
    pub obligations: ObligationState,
    pub inventory: InventoryState,
    pub exit: ExitState,
    pub stdout: PipeState,
    pub stderr: PipeState,
    pub input: InputState,
    pub capture: CaptureState,
    pub watermark: WatermarkState,
    pub containment: ContainmentState,
    pub writer: WriterState,
    pub claims: ClaimsState,
    pub execution: ExecutionState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationDecision {
    pub result: ResultState,
    pub workflow: WorkflowState,
    pub release: ReleaseEvidence,
    pub reason: &'static str,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelDecision {
    pub permitted: bool,
    pub reason: &'static str,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeIdentity {
    pub sequence: u64,
    pub operation: Id,
    pub generation: Id,
    pub attempt: Id,
    pub channel_nonce: Id,
    pub writer: ProcessIdentity,
    pub binding_digest: Digest,
    pub command_digest: Digest,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingExchange {
    pub identity: ExchangeIdentity,
    pub command: StorageModelCommand,
    pub expected_revision: u64,
    pub payload_bytes: usize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AckExchange {
    pub identity: ExchangeIdentity,
    pub revision: Option<u64>,
    pub state: AckState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeObservation {
    pub pending: PendingExchange,
    pub ack: AckExchange,
    pub send_state: SendState,
    pub deadline: DeadlineState,
    pub poisoned: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityContext {
    pub caller: ProcessIdentity,
    pub role: AuthorityRole,
    pub checkpoint: Option<Id>,
    pub revision: Option<u64>,
    pub issued_attempt: Id,
    pub grant_digest: Digest,
}
/// Static preparation for the closed unpaid mechanics fixture, never a public run grant.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparationEnvelope {
    pub profile_digest: Digest,
    pub policy: RuntimePolicy,
    pub configuration_digest: Digest,
    pub operation: Id,
    pub session: Id,
    pub reservation: Id,
    pub attempt: Id,
    pub generation: Id,
    pub authority: AuthorityContext,
    pub assignment: FixtureAssignment,
}

//! Candidate policy values only; neither a model decision nor an ID grants authority.
mod values;
pub use values::{
    AckExchange, AuthorityContext, ExchangeIdentity, ExchangeObservation, ModelDecision,
    ObservationDecision, PendingExchange, PermissionObservation, PreparationEnvelope,
    RuntimeObservation, RuntimePolicy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement {
    Required,
    Incidental,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionMode {
    OsEnforced,
    ToolLevel,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationState {
    Passed,
    Failed,
    Unknown,
    Unmeasured,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchState {
    Proven,
    Unknown,
    Mismatch,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultState {
    Unknown,
    Succeeded,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultBinding {
    Matched,
    Mismatched,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObligationState {
    Pending,
    Satisfied,
    Failed,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryState {
    Complete,
    Partial,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitState {
    Unobserved,
    Zero,
    Nonzero,
    Signal,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipeState {
    Eof,
    Open,
    Unknown,
    Error,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputState {
    Reconciled,
    Pending,
    Unknown,
    Error,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureState {
    Lossless,
    KnownLoss,
    Unknown,
    Error,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatermarkState {
    Complete,
    Gap,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainmentState {
    Complete,
    Incomplete,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterState {
    Settled,
    Live,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimsState {
    Reconciled,
    Pending,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionState {
    Healthy,
    Failed,
    Cancelled,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowState {
    Blocked,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseEvidence {
    Blocked,
    EvidenceComplete,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendState {
    Complete,
    Partial,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeadlineState {
    Current,
    Expired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AckState {
    Committed,
    Refused,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageModelCommand {
    Bootstrap,
    Claim,
    RecordChild,
    CreateRaw,
    Accept,
    CaptureBegin,
    Append,
    KnownLoss,
    Finish,
    Close,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityRole {
    Direct,
    Worker,
    Builder,
    Orchestrator,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureAssignment {
    FixedEffect,
    SaturatedEffect,
}

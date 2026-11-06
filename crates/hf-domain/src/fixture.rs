//! Private fixed-fixture handoff values; not an engine execution interface.
use crate::{
    ownership::{Digest, Fence, Id, PreparedRequest},
    process::ProcessIdentity,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    ParentBeforePrepare,
    ParentAfterPrepare,
    ParentBeforeSpawn,
    ParentAfterOwnerSpawn,
    OwnerAfterClaim,
    OwnerAfterChildSpawn,
    OwnerAfterObserved,
    OwnerBeforeAck,
    CaptureBeforeSync,
    CaptureAfterSync,
    CaptureAfterPublish,
    CaptureBeforeTerminal,
    CaptureAfterTerminal,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    pub stage: Stage,
    pub path: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invitation {
    pub operation: Id,
    pub generation: Id,
    pub digest: Digest,
    pub owner: Id,
    pub nonce: Id,
    pub process: ProcessIdentity,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub invitation: Invitation,
    pub request: PreparedRequest,
    pub root: String,
    pub gate: Option<Gate>,
    pub ledger: QualifiedLedger,
    pub fault: FixtureFault,
}
/// Deny-only fixture injection, never a backend or recovery option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureFault {
    None,
    UnknownChildIdentity,
    UnusableChildPipe,
    OwnerCapacityExhausted,
    OutputSaturation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualifiedLedger {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentPlan {
    pub root: String,
    pub request: PreparedRequest,
    pub initialize: bool,
    pub launch: bool,
    pub owner: Id,
    pub nonce: Id,
    pub gate: Option<Gate>,
    pub ledger: QualifiedLedger,
    pub fault: FixtureFault,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectStart {
    pub operation: Id,
    pub root: String,
    pub ledger: QualifiedLedger,
    pub fence: Fence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    Prepared,
    Existing,
    Refused,
    Uncertain,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello(ProcessIdentity),
    CaptureAccepted(Box<crate::prepared_launch::CaptureAccepted>),
    Invitation(Invitation),
    Ready(Invitation),
    Prepared(Prepared),
    Accepted {
        invitation: Invitation,
        fence: Fence,
        child: ProcessIdentity,
    },
    Parent(ParentPlan),
    Start(EffectStart),
    EffectDone {
        operation: Id,
        process: ProcessIdentity,
    },
    Report {
        operation: Id,
        generation: Id,
        kind: ReportKind,
    },
}

//! Private offline fixture Queue ledger. No provider or production authority.
mod lifecycle;
mod retention;
mod transitions;
mod validation;
use crate::{
    ownership::{Digest, Fence, Id, PreparedRequest},
    process::ProcessIdentity,
};
pub const MAX_TEXT: usize = 32 * 1024;
pub const MAX_PROSE: usize = 1024 * 1024;
pub const MAX_OUTSTANDING: usize = 32;
pub const MAX_RECEIPTS: usize = 64;
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
pub const COMPLETION_RESERVE: usize = 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
    pub conversation: Id,
    pub head_task: Id,
    pub source: PreparedRequest,
    pub fence: Fence,
    pub child: ProcessIdentity,
    pub context: Id,
    pub profile: Digest,
    pub generation: u64,
    pub turn: Id,
    pub ordinary_turns: u64,
    pub node_attempts: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Witness {
    pub conversation: Id,
    pub head_task: Id,
    pub source_operation: Id,
    pub generation: u64,
    pub turn: Id,
    pub ledger_epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendIntent {
    pub request: Id,
    pub expected: Witness,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelIntent {
    pub request: Id,
    pub message: Id,
    pub expected: Witness,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimIntent {
    pub request: Id,
    pub expected: Witness,
    pub destination: Id,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageState {
    Queued,
    Claimed,
    Consumed,
    Unknown,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub intent: ClaimIntent,
    pub message: Id,
    pub source: Id,
    pub context: Id,
    pub generation: u64,
    pub turn: Id,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub claim: Claim,
    pub evidence: Id,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub intent: SendIntent,
    pub sequence: u64,
    pub state: MessageState,
    pub claim: Option<Claim>,
    pub acknowledged: bool,
    pub receipt: Option<Receipt>,
    pub cancellation: Option<CancelIntent>,
}
/// Supplied only by the application's injected trusted offline fixture oracle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundaryEvidence {
    pub expected: Witness,
    pub context: Id,
    pub profile: Digest,
    pub evidence: Id,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    pub enrollment: Enrollment,
    pub generation: u64,
    pub turn: Id,
    pub ledger_epoch: u64,
    pub boundary: Option<BoundaryEvidence>,
    pub closed: bool,
    pub runtime_fault: bool,
    pub next_sequence: u64,
    pub cumulative_issuances: u64,
    pub unavailable_usage: u64,
    /// One past the highest retired sequence. Protected entries may remain
    /// retained below it, because retirement is not strictly oldest-first.
    pub history_floor: u64,
    pub retired_messages: u64,
    pub history_gaps: bool,
    pub messages: Vec<Message>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Conflict,
    Stale,
    Expired,
    Closed,
    Fault,
    Capacity,
    Blocked,
    AlreadyClaimed,
    Missing,
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, PartialEq, Eq)]
pub struct ClaimOutcome<G> {
    pub ledger: Ledger,
    pub grant: Option<G>,
}

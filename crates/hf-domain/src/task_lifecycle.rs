//! Durable execution observations. None of these values grants settlement or release.
use crate::{
    ownership::{Digest, Id, OwnerClaim, Snapshot},
    process::{ProcessExit, ProcessIdentity},
    task_read::TaskId,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleBinding {
    pub task: TaskId,
    pub operation: Id,
    pub attempt: Id,
    pub generation: Id,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelIntent {
    pub sequence: u64,
    pub reason: String,
    pub accepted_at: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    Unknown,
    Partial,
    Complete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    Incidental,
    Required,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultWitness {
    pub turn: Id,
    pub digest: Digest,
    pub source_end: u64,
    pub success: bool,
}
/// Backend-protocol provenance; it never derives from an OS scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObligationWitness {
    pub turn: Id,
    pub record_digest: Digest,
    pub source_start: u64,
    pub source_end: u64,
    pub observed_items: Vec<ObligationItem>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObligationItem {
    pub id: Id,
    pub failed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionObservation {
    /// Serialized retained normalized bytes, maintained by the atomic writer.
    pub normalized_event_bytes: u64,
    pub omitted_events: u64,
    pub result: Option<ResultWitness>,
    pub exit: ProcessExit,
    pub drained: bool,
    pub input_closed: bool,
    pub capture_known_loss: bool,
    pub stdout_end: u64,
    pub stderr_end: u64,
    pub observation_gap: bool,
    pub event_projection_truncated: bool,
    pub obligations: Completeness,
    pub obligation_witness: Option<ObligationWitness>,
    pub requirement: Requirement,
    /// A bounded OS sample can never establish complete containment.
    pub descendants: Vec<ProcessIdentity>,
    pub descendant_scan: Option<Id>,
}
impl Default for ExecutionObservation {
    fn default() -> Self {
        Self {
            normalized_event_bytes: 0,
            omitted_events: 0,
            result: None,
            exit: ProcessExit::Unobserved,
            drained: false,
            input_closed: false,
            capture_known_loss: false,
            stdout_end: 0,
            stderr_end: 0,
            observation_gap: false,
            event_projection_truncated: false,
            obligations: Completeness::Unknown,
            obligation_witness: None,
            requirement: Requirement::Incidental,
            descendants: Vec::new(),
            descendant_scan: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleEvidence {
    pub binding: LifecycleBinding,
    pub revision: u64,
    pub ownership: Snapshot,
    pub cancel: Option<CancelIntent>,
    pub observation: ExecutionObservation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleDelta {
    OwnerClaimed(OwnerClaim),
    ChildRecorded(ProcessIdentity),
    CancelCommitted(CancelIntent),
    ObservationCommitted(ExecutionObservation),
    ExecutionObserved(ExecutionObservation),
}

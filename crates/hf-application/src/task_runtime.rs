//! Trusted canonical admission ports for private intent and first public projection.
use crate::task_lifecycle::InitialProjectionFacts;
use hf_domain::{
    ownership::PreparedRequest,
    process::ProcessIdentity,
    task_runtime::{LaunchIntent, LaunchIntentCreate},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidIntent;
pub trait InitialIntentValidator {
    fn validate(
        &self,
        request: &PreparedRequest,
        intent: &LaunchIntentCreate,
    ) -> Result<(), InvalidIntent>;
}
pub struct SpawnProjectionFacts<'a> {
    pub initial: InitialProjectionFacts<'a>,
    pub child: &'a ProcessIdentity,
}
pub trait SpawnProjectionValidator {
    fn validate(
        &self,
        intent: &LaunchIntent,
        facts: &SpawnProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
        prompt: &str,
    ) -> Result<(), InvalidIntent>;
}
/// Trusted writer-held capability checks actual child, live control and raw-source readiness.
/// Admission must be bounded/read-only and must not acquire storage or filesystem locks.
pub trait AcceptanceAdmission {
    fn admit(
        &self,
        evidence: &hf_domain::task_lifecycle::LifecycleEvidence,
        readiness: &hf_domain::task_runtime::AcceptanceReadiness,
    ) -> Result<(), InvalidIntent>;
}
pub struct TerminalProjectionFacts<'a> {
    pub initial: InitialProjectionFacts<'a>,
    pub exit_code: i64,
    pub finished_at: &'a str,
    pub output_drained: bool,
}
pub trait TerminalProjectionValidator {
    fn validate(
        &self,
        evidence: &hf_domain::task_lifecycle::LifecycleEvidence,
        facts: &TerminalProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
        event_body: &[u8],
        event_seq: u64,
    ) -> Result<(), InvalidIntent>;
}
/// Original-owner proof of wait, actual EOF, reader join/FIFO and input reconciliation.
/// No caller-supplied Boolean or result record is sufficient to implement this capability.
pub trait TerminalAdmission {
    fn admit(
        &self,
        evidence: &hf_domain::task_lifecycle::LifecycleEvidence,
        observation: &hf_domain::task_lifecycle::ExecutionObservation,
    ) -> Result<(), InvalidIntent>;
}
/// Strict production preparation decoder for deny-only root participant inventory.
/// Unknown/corrupt/legacy preparation must refuse rather than imply no agent.
pub trait RuntimeParticipantDecoder {
    fn participants(
        &self,
        request: &PreparedRequest,
    ) -> Result<Vec<ProcessIdentity>, InvalidIntent>;
}
/// Positive original-owner proof that no agent started and no uncertain probe remains.
pub trait PreAgentRefusalAdmission {
    fn admit(
        &self,
        evidence: &hf_domain::task_lifecycle::LifecycleEvidence,
        reason: &str,
    ) -> Result<(), InvalidIntent>;
}

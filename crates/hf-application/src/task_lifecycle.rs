//! Initial served-projection admission port; serialization belongs to interfaces.
use hf_domain::task_read::TaskId;
#[derive(Clone, Copy, Debug)]
pub struct InitialProjectionFacts<'a> {
    pub task: &'a TaskId,
    pub backend: &'a str,
    pub repo_path: &'a str,
    pub session_id: &'a str,
    pub status: &'a str,
    pub started_at: &'a str,
    pub title: Option<&'a str>,
    pub model: Option<&'a str>,
    pub continuation_of: Option<&'a TaskId>,
    pub chain_root: Option<&'a str>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidInitialProjection;
/// Trusted composition injects the canonical interfaces implementation, never a caller claim.
pub trait InitialProjectionValidator {
    fn validate(
        &self,
        facts: &InitialProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
    ) -> Result<(), InvalidInitialProjection>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureProgress {
    pub stdout_end: u64,
    pub stderr_end: u64,
    pub known_loss: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct DurableCaptureRequest<'a> {
    pub binding: &'a hf_domain::task_lifecycle::LifecycleBinding,
    pub child: &'a hf_domain::process::ProcessIdentity,
    pub previous: CaptureProgress,
    pub next: CaptureProgress,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureAdmissionRefused;
/// Required trusted writer-held raw-capture capability. An implementation must verify the
/// exact root/attempt/actual child, source identity, synced offsets and durable known-loss
/// evidence. Caller-supplied numbers are not proof. Admission is bounded and read-only;
/// it must not mutate the store or reacquire its writer lock. No production implementation
/// is supplied by this foundation; public T3 remains closed until owner composition qualifies it.
pub trait DurableCaptureAdmission {
    fn admit(&self, request: &DurableCaptureRequest<'_>) -> Result<(), CaptureAdmissionRefused>;
}

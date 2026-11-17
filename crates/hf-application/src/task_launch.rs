//! Injected production preparation boundaries; no process effects or transport dependency.
use hf_domain::task_launch::{DiscoveredClaude, LaunchRefusal, LaunchSpec, PreparedInvocation};
use std::time::Instant;

pub trait ClaudeDiscoveryPort {
    fn discover(&mut self, deadline: Instant) -> Result<DiscoveredClaude, LaunchRefusal>;
}
/// The host supplies the canonical transport codec. Backends never depend on interfaces.
pub trait LiveInputCodecPort {
    fn encode_user_line(&self, prompt: &str) -> Result<Vec<u8>, LaunchRefusal>;
}
pub trait BackendPreparationPort {
    fn prepare(
        &mut self,
        spec: &LaunchSpec,
        deadline: Instant,
    ) -> Result<PreparedInvocation, LaunchRefusal>;
}

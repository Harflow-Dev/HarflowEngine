//! Normal Claude preparation, distinct from closed offline/qualification routes.
// Evidence: HarflowMCP b475c408fe9feb84409cf147550d1436030789d5,
// src/harflow_mcp/backends/claude.py::_common/_invocation/encode_live_message.
// docs/backend-cli-facts.md records live JSONL input and bypassPermissions
// independently; conformance/claude_fake/recordings pins CLI 2.1.322. This
// composition is the canonical Python production shape, not a claim that the
// exact unrestricted/live combination has a new real-provider qualification.
use hf_application::task_launch::{
    BackendPreparationPort, ClaudeDiscoveryPort, LiveInputCodecPort,
};
use hf_domain::task_launch::*;
use std::time::Instant;

// Recorded CLI version, not an executable hash or test-backend selector.
pub const CLAUDE_VERSION: &str = "2.1.322";
pub const MAX_PROMPT_BYTES: usize = 1_048_576;

pub struct ClaudePreparation<D, C> {
    discovery: D,
    codec: C,
}
impl<D, C> ClaudePreparation<D, C> {
    pub fn new(discovery: D, codec: C) -> Self {
        Self { discovery, codec }
    }
    pub fn discovery(&mut self) -> &mut D {
        &mut self.discovery
    }
}
impl<D: ClaudeDiscoveryPort, C: LiveInputCodecPort> BackendPreparationPort
    for ClaudePreparation<D, C>
{
    fn prepare(
        &mut self,
        spec: &LaunchSpec,
        deadline: Instant,
    ) -> Result<PreparedInvocation, LaunchRefusal> {
        prepare_claude(spec, &mut self.discovery, &self.codec, deadline)
    }
}
pub fn validate_spec(spec: &LaunchSpec) -> Result<(), LaunchRefusal> {
    if spec.permission != LaunchPermission::Unrestricted {
        return Err(LaunchRefusal::UnsupportedPermission);
    }
    if !spec.options.is_empty() {
        return Err(LaunchRefusal::UnsupportedOption);
    }
    if spec.prompt.trim().is_empty()
        || spec.prompt.len() > MAX_PROMPT_BYTES
        || spec.prompt.contains('\0')
    {
        return Err(LaunchRefusal::InvalidPrompt);
    }
    if !valid_path(&spec.cwd) {
        return Err(LaunchRefusal::InvalidDirectory);
    }
    Ok(())
}
pub fn prepare_claude(
    spec: &LaunchSpec,
    discovery: &mut impl ClaudeDiscoveryPort,
    codec: &impl LiveInputCodecPort,
    deadline: Instant,
) -> Result<PreparedInvocation, LaunchRefusal> {
    validate_spec(spec)?;
    if Instant::now() >= deadline {
        return Err(LaunchRefusal::Deadline);
    }
    let initial_input = codec.encode_user_line(&spec.prompt)?;
    if initial_input.is_empty()
        || initial_input.len() > MAX_PROMPT_BYTES * 6 + 256
        || initial_input.last() != Some(&b'\n')
        || initial_input[..initial_input.len() - 1].contains(&b'\n')
        || initial_input.contains(&0)
    {
        return Err(LaunchRefusal::Codec);
    }
    let executable = discovery.discover(deadline)?;
    validate_discovery(&executable)?;
    if Instant::now() >= deadline {
        return Err(LaunchRefusal::Deadline);
    }
    Ok(PreparedInvocation {
        session_id: spec.session_id.clone(),
        executable,
        args: [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-mode",
            "bypassPermissions",
            "--input-format",
            "stream-json",
            "--session-id",
            spec.session_id.as_str(),
        ]
        .iter()
        .map(|s| (*s).into())
        .collect(),
        initial_input,
        cwd: spec.cwd.clone(),
        stdin: PreparedStdin::LivePipe,
    })
}
fn validate_discovery(value: &DiscoveredClaude) -> Result<(), LaunchRefusal> {
    if value.version != CLAUDE_VERSION {
        return Err(LaunchRefusal::UnsupportedVersion);
    }
    let binary = &value.binary;
    if !valid_path(&binary.logical_path)
        || !valid_path(&binary.physical_path)
        || binary.metadata.identity.inode == 0
        || binary.metadata.length == 0
        || binary.logical_path.rsplit('/').next() != Some("claude")
    {
        return Err(LaunchRefusal::InvalidDiscovery);
    }
    Ok(())
}
fn valid_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && !value.split('/').any(|part| part == "." || part == "..")
}

//! Production launch preparation values. Preparation alone grants no execution authority.
use crate::prepared_launch::BinaryPin;

/// An admitted CLI session identity; generation and durable task binding belong
/// to the host/application launch admission, never to the backend adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliSessionId(String);
impl CliSessionId {
    pub fn new(value: String) -> Result<Self, LaunchRefusal> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
        {
            return Err(LaunchRefusal::InvalidSession);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchPermission {
    Unrestricted,
    Unsupported(String),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchSpec {
    pub session_id: CliSessionId,
    pub prompt: String,
    pub cwd: String,
    pub permission: LaunchPermission,
    /// Meaningful options outside this checkpoint are explicitly refused.
    pub options: Vec<String>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchRefusal {
    UnsupportedPermission,
    UnsupportedOption,
    InvalidPrompt,
    InvalidSession,
    InvalidDirectory,
    InvalidDiscovery,
    NotFound,
    UnsupportedVersion,
    Unavailable,
    IdentityChanged,
    Deadline,
    ProbeUnsettled,
    Codec,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredClaude {
    /// Logical PATH selection and independently observed physical identity.
    pub binary: BinaryPin,
    pub version: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedStdin {
    /// Write the first user line, retaining the pipe for owner-controlled EOF.
    LivePipe,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedInvocation {
    pub session_id: CliSessionId,
    pub executable: DiscoveredClaude,
    /// Excludes argv[0]. Never contains the user prompt.
    pub args: Vec<String>,
    pub initial_input: Vec<u8>,
    pub cwd: String,
    pub stdin: PreparedStdin,
}

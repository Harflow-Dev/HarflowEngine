//! Offline observations only; no value grants process or production authority.
use crate::{
    capture::FileIdentity,
    owner_policy::{AuthorityContext, ResultState, RuntimePolicy},
    ownership::{Digest, Id, PreparedRequest},
};
use std::{string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq)]
pub struct BackendBinding {
    pub request: PreparedRequest,
    pub authority: AuthorityContext,
    pub root: FileIdentity,
    pub profile: Digest,
    pub policy: RuntimePolicy,
    pub runtime_version: String,
    pub logical_executable: String,
    pub physical_executable: String,
    pub binary_digest: Digest,
    pub configuration_digest: Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendMode {
    OfflineOneShot,
    Live,
    Resume,
    Production,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    Unsupported,
    InvalidBinding,
    Budget,
    Shape,
    Truncated,
    Session,
    Correlation,
    PostTerminal,
    Source,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Init,
    Metadata,
    Text(String),
    ToolUse(String),
    ToolResult { id: String, is_error: bool },
    Terminal { failed: bool },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendRecord {
    pub cli_session: String,
    pub attempt: Id,
    pub parent: Option<String>,
    pub kind: RecordKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendTrace {
    pub records: Vec<BackendRecord>,
    pub fault: Option<BackendError>,
    pub ended: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BackendSummary {
    pub binding: BackendBinding,
    pub result: ResultState,
    pub sticky_failure: bool,
    pub usable: bool,
    pub terminal: bool,
    pub pending_tools: usize,
    pub text: String,
    pub cli_session: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationPlan {
    pub executable: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    pub source_profile: String,
    pub configuration_digest: Digest,
}

impl BackendError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Unsupported => {
                "Only recorded offline one-shot observations are supported; live input, resume, backend launch and production admission require separately accepted contracts and qualification."
            }
            Self::InvalidBinding => {
                "Declared request, caller, grant, runtime, policy or invocation configuration does not match the bounded offline context."
            }
            Self::Budget => "The bounded offline observation budget was exceeded.",
            Self::Shape => "The observation is not a supported strict JSON event shape.",
            Self::Truncated => "The recorded JSONL source ended inside an unterminated record.",
            Self::Session => "The CLI session differs from the recorded root session.",
            Self::Correlation => {
                "The observation does not correlate with this attempt or tool scope."
            }
            Self::PostTerminal => "The one-shot source contains activity after a root terminal.",
            Self::Source => {
                "The finite recorded source is unavailable; it must not be retried as a live operation."
            }
        }
    }
}

//! Closed diagnostic composition, never production admission or root authority.
use crate::{
    capture::FileIdentity, fixture::QualifiedLedger, owner_policy::PreparationEnvelope,
    ownership::Digest, prepared_launch::BinaryPin, recovery::SourceStamp,
};
pub const VENDOR_VERSION: &str = "2.1.318";
pub const VENDOR_PATH: &str = "/Users/nayanda/.local/share/claude/versions/2.1.318";
pub const VENDOR_DIGEST: &str = "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d";
pub const VENDOR_LENGTH: u64 = 319598464;
pub const VENDOR_READ_LIMIT: u64 = 512 * 1024 * 1024;
pub const PARENT_MODE: &str = "--fixture-claude-qualification-parent";
pub const OWNER_MODE: &str = "--fixture-claude-qualification-owner";
pub const WRITER_MODE: &str = "--fixture-claude-qualification-writer";
pub const BRIDGE_MODE: &str = "--fixture-claude-qualification-bridge";
pub const PARENT_SECONDS: u64 = 130;
pub const OWNER_SECONDS: u64 = 100;
pub const VENDOR_SECONDS: u64 = 90;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedDirectory {
    pub path: String,
    pub identity: FileIdentity,
}
/// Deny-only observations. This is never a reconstructed FixtureRoot authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedFixtureTopology {
    pub root: RecordedDirectory,
    pub roles: Vec<RecordedDirectory>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationCommand {
    pub executable: String,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeQualification {
    pub policy: PreparationEnvelope,
    pub gate_binary: BinaryPin,
    pub vendor_binary: BinaryPin,
    pub vendor_version: String,
    pub logical_hint: String,
    pub cwd: String,
    pub source: SourceStamp,
    pub ledger: QualifiedLedger,
    pub topology: RecordedFixtureTopology,
    pub nonce: String,
    pub port: u16,
    pub profile: Vec<u8>,
    pub profile_digest: Digest,
    pub command: QualificationCommand,
}

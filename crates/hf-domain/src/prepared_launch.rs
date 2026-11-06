//! Closed private helper preparation and volatile historical capture evidence.
use crate::{
    capture::{CaptureSnapshot, FileIdentity},
    fixture::{Invitation, QualifiedLedger},
    owner_policy::PreparationEnvelope,
    ownership::{Digest, Snapshot},
    recovery::{RecoveryRecord, SourceStamp},
};
pub const FIXED_PROTOCOL_ARG: &str = "--fixture-fixed-protocol-helper";
pub const SOURCE_PROFILE: &str = "fixed-protocol-helper-v1";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchAssignment {
    FixedProtocolHelper,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryMetadata {
    pub identity: FileIdentity,
    pub length: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub changed_seconds: i64,
    pub changed_nanoseconds: i64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryPin {
    pub logical_path: String,
    pub physical_path: String,
    pub metadata: BinaryMetadata,
    pub digest: Digest,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedLaunch {
    pub policy: PreparationEnvelope,
    pub assignment: LaunchAssignment,
    pub binary: BinaryPin,
    pub argv: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub cwd: String,
    pub source: SourceStamp,
    pub ledger: QualifiedLedger,
    pub source_profile: String,
}
/// Obtained only from the original private handoff; stored rows cannot reconstruct it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureAccepted {
    pub invitation: Invitation,
    pub source: SourceStamp,
    pub ownership: Snapshot,
    pub capture: CaptureSnapshot,
    pub stdout_digest: Digest,
    pub stderr_digest: Digest,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawBytes {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedSource {
    pub receipt: CaptureAccepted,
    pub bytes: RawBytes,
    pub record: RecoveryRecord,
    pub framed_digest: Digest,
}

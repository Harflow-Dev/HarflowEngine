//! Private runtime messages. Values convey no caller, release or OS authority.
use crate::{
    capture::{CaptureBinding, CaptureSnapshot, FileIdentity},
    ownership::{Digest, Id, OwnerClaim, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
    recovery::SourceStamp,
};
use std::boxed::Box;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageParticipant {
    pub source: SourceStamp,
    pub operation: Id,
    pub attempt: Id,
    pub request_digest: Digest,
    pub configuration_digest: Digest,
    pub profile_digest: Digest,
    pub owner: OwnerClaim,
    pub writer: ProcessIdentity,
    pub channel_nonce: Id,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}
/// Trusted owner-loop facts, never decoded from optional public JSON fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedDrainEvidence {
    pub reader_joined: bool,
    pub accepted_data_reconciled: bool,
    pub input_reconciled: bool,
    pub stdout_eof: bool,
    pub stderr_eof: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageCommand {
    Bootstrap(Box<StorageBootstrap>),
    Claim,
    RecordChild(ProcessIdentity),
    CreateRaw,
    Accept,
    CaptureBegin(Box<CaptureBinding>),
    Append {
        stream: Stream,
        bytes: Vec<u8>,
        drain: Option<TrustedDrainEvidence>,
    },
    KnownLoss,
    Finish(ProcessExit),
    Close,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandIdentity {
    pub sequence: u64,
    pub binding_digest: Digest,
    pub command_digest: Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckDisposition {
    Committed,
    Refused,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageAck {
    pub identity: CommandIdentity,
    pub disposition: AckDisposition,
    pub revision: Option<u64>,
    pub snapshot: Option<Snapshot>,
    pub capture: Option<CaptureSnapshot>,
    pub raw_files: Option<RawIdentities>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageBootstrap {
    pub root: String,
    pub request: PreparedRequest,
    pub participant: StorageParticipant,
    pub gate: Option<crate::fixture::Gate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawIdentities {
    pub stdout: FileIdentity,
    pub stderr: FileIdentity,
}
impl RawIdentities {
    pub fn is_valid(&self) -> bool {
        self.stdout.inode != 0 && self.stderr.inode != 0 && self.stdout != self.stderr
    }
}

use super::{support::*, *};
use hf_application::ctl::state_recover::{evidence::OwnerEvidencePort, flow::RecoveryError};
use hf_domain::ctl::recovery_proposal::RecoveryProposal;
use hf_platform::ctl::archive_fs::ArchiveFaultPort;

impl Fixture {
    pub fn complete(
        &self,
        p: &RecoveryProposal,
        s: &MaintenanceSession,
        faults: &dyn ArchiveFaultPort,
        evidence: &dyn OwnerEvidencePort,
        initializer: &dyn hf_application::ctl::state_recover::initialization::FreshStorePort,
    ) -> Result<hf_application::ctl::state_recover::journal::RecoveryResult, RecoveryError> {
        use hf_application::ctl::state_recover::{completion::CompletionPorts, flow};
        use hf_platform::ctl::archive_fs::{FsRecovery, FsRecoveryRead};
        let root = self.root();
        let archive = FsArchive::new(s, &MacExclusiveRename, faults, &JsonJournalCodec);
        let staging = FsRecovery::new(
            &archive,
            initializer,
            &Sha256Digest,
            &hf_platform::durability::FullFlush,
        );
        flow::recover(
            &flow::RecoveryRequest {
                root: &root,
                proposal: p,
                operation_id: &p.operation_id,
                created_at: "2026-11-16T00:00:00Z",
            },
            &flow::RecoveryPorts {
                session: s,
                evidence,
                observer: &FsStateObserver,
                state_codec: &JsonStateCodec,
                policy: policy(),
                inventory: &FsInventory,
                inventory_codec: &JsonRecoveryCodec,
                digest: &Sha256Digest,
                identities: &hf_platform::process::MacIdentity,
                journal_codec: &JsonJournalCodec,
                mutations: &archive,
            },
            &CompletionPorts {
                read: &FsRecoveryRead::new(&root, &JsonStateCodec),
                mutations: &archive,
                staging: &staging,
            },
        )
    }
}
pub struct NoInitialize;
impl hf_application::ctl::state_recover::initialization::FreshStorePort for NoInitialize {
    fn initialize(
        &self,
        _: &Path,
        _: hf_domain::ctl::recovery_proposal::FileIdentity,
        _: Instant,
    ) -> Result<(), RecoveryError> {
        panic!("staged resume reopened SQLite")
    }
}

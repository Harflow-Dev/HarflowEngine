//! Internal archive-only recovery. A successful return is a pending B5 handoff.
mod admission;
mod run;
use super::{
    binding::HeldSessionPort,
    evidence::OwnerEvidencePort,
    journal::{JournalError, RecoveryJournal},
};
use crate::{
    ctl::state_inspect::{
        InventoryPort, RecoveryCodecPort, StateCodecPort, StateObserverPort, StorePolicy,
    },
    digest::DigestPort,
    process::IdentityPort,
};
use hf_domain::ctl::{
    recovery_proposal::{FileIdentity, InventoryEntry, RecoveryProposal},
    state_root::StateRoot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryError {
    StaleConfirmation,
    ActiveOwner,
    AmbiguousOwner,
    UnsafeRoot,
    UnsupportedLayout,
    BackupConflict,
    InfrastructureUnavailable,
    RecoveryInterrupted,
}

pub trait JournalCodecPort {
    fn encode(&self, journal: &RecoveryJournal) -> Result<Vec<u8>, JournalError>;
    fn decode(&self, bytes: &[u8]) -> Result<RecoveryJournal, JournalError>;
}

/// Effects run under the original session. Implementations fence root/locks and the
/// journaled backup/data identities before every mutation. Every write, flush,
/// rename and directory sync must have its own injectable fault boundary.
pub trait ArchiveMutationPort {
    fn staging_absent(&self, operation_id: &str) -> Result<(), RecoveryError>;
    fn backup_absent(&self, path: &str) -> Result<(), RecoveryError>;
    fn write_pending(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn write_journal(&self, journal: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError>;
    /// Creates or qualifies Python-r4's partial skeleton; persists previous bytes and
    /// the exact Prepared manifest before returning its directory identities.
    fn prepare_backup(
        &self,
        journal: &RecoveryJournal,
        codec: &dyn JournalCodecPort,
    ) -> Result<(FileIdentity, FileIdentity), RecoveryError>;
    /// Verifies identities and all entries in exactly one location, their full subtree
    /// attributes/link targets, and no unexpected entry. `final_only` requires data only.
    fn validate_locations(
        &self,
        journal: &RecoveryJournal,
        final_only: bool,
    ) -> Result<(), RecoveryError>;
    fn move_entry(
        &self,
        journal: &RecoveryJournal,
        name: &str,
        expected: &InventoryEntry,
    ) -> Result<(), RecoveryError>;
    fn move_shm(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn write_manifest(&self, journal: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError>;
}

pub struct RecoveryRequest<'a> {
    pub root: &'a StateRoot,
    pub proposal: &'a RecoveryProposal,
    pub operation_id: &'a str,
    pub created_at: &'a str,
}
pub struct RecoveryPorts<'a> {
    pub session: &'a dyn HeldSessionPort,
    pub evidence: &'a dyn OwnerEvidencePort,
    pub observer: &'a dyn StateObserverPort,
    pub state_codec: &'a dyn StateCodecPort,
    pub policy: StorePolicy,
    pub inventory: &'a dyn InventoryPort,
    pub inventory_codec: &'a dyn RecoveryCodecPort,
    pub digest: &'a dyn DigestPort,
    pub identities: &'a dyn IdentityPort,
    pub journal_codec: &'a dyn JournalCodecPort,
    pub mutations: &'a dyn ArchiveMutationPort,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackedUpHandoff {
    pub journal: RecoveryJournal,
}

pub fn recover(
    request: &RecoveryRequest<'_>,
    ports: &RecoveryPorts<'_>,
) -> Result<BackedUpHandoff, RecoveryError> {
    let (journal, fresh) = admission::admit(request, ports)?;
    run::archive(journal, fresh, ports).map(|journal| BackedUpHandoff { journal })
}

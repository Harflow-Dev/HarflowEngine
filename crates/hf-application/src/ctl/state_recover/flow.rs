//! Internal recovery orchestration with a separately callable archive helper.
pub use super::failure::{RecoveryFailure, RecoveryPhase, RecoveryProgress};
pub(super) mod admission;
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
    PythonState,
    UnsupportedLayout,
    BackupConflict,
    InfrastructureUnavailable,
    RecoveryInterrupted,
}

pub trait JournalCodecPort {
    fn encode(&self, journal: &RecoveryJournal) -> Result<Vec<u8>, JournalError>;
    fn decode(&self, bytes: &[u8]) -> Result<RecoveryJournal, JournalError>;
    fn encode_result(
        &self,
        result: &super::journal::RecoveryResult,
    ) -> Result<Vec<u8>, JournalError>;
    fn decode_result(&self, bytes: &[u8]) -> Result<super::journal::RecoveryResult, JournalError>;
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

/// Archive helper; production recovery continues through initialization and completion.
pub fn archive_only(
    request: &RecoveryRequest<'_>,
    ports: &RecoveryPorts<'_>,
) -> Result<BackedUpHandoff, RecoveryError> {
    let (journal, fresh) = admission::admit(request, ports)?;
    run::archive(journal, fresh, ports).map(|journal| BackedUpHandoff { journal })
}

/// Complete internal recovery; historical receipts must be queried before session acquisition.
pub fn recover(
    request: &RecoveryRequest<'_>,
    ports: &RecoveryPorts<'_>,
    completion: &super::completion::CompletionPorts<'_>,
) -> Result<super::journal::RecoveryResult, RecoveryError> {
    recover_qualified(request, ports, completion).map_err(|failure| failure.error)
}

fn interrupted(error: RecoveryError) -> RecoveryError {
    match error {
        RecoveryError::InfrastructureUnavailable => RecoveryError::RecoveryInterrupted,
        other => other,
    }
}

/// Public composition uses this path so errors retain only admitted protocol facts.
pub fn recover_qualified(
    request: &RecoveryRequest<'_>,
    ports: &RecoveryPorts<'_>,
    completion: &super::completion::CompletionPorts<'_>,
) -> Result<super::journal::RecoveryResult, RecoveryFailure> {
    let mut progress = None;
    let committed = super::completion::committed_tracked(request, ports, completion, &mut progress)
        .map_err(|error| RecoveryFailure {
            error,
            progress: progress.clone(),
        })?;
    let mut journal = if let Some(journal) = committed {
        journal
    } else {
        let (mut journal, fresh) = admission::admit_tracked(request, ports, &mut progress)
            .map_err(|error| RecoveryFailure {
                error,
                progress: progress.clone(),
            })?;
        if journal.phase != super::journal::JournalPhase::BackedUp {
            run::archive_tracked(&mut journal, fresh, ports)
                .map_err(|error| RecoveryFailure::journal(error, &journal))?;
        }
        super::completion::initialize_tracked(&mut journal, ports, completion)
            .map_err(|error| RecoveryFailure::journal(interrupted(error), &journal))?;
        journal
    };
    // Preserve qualified entry evidence: constructing a completed document is not
    // publication of a completed receipt. Fresh R9 failures still report committed.
    let entry_phase = if journal.phase == super::journal::JournalPhase::Completed {
        RecoveryPhase::Completed
    } else {
        RecoveryPhase::Committed
    };
    super::completion::finish_tracked(&mut journal, ports, completion).map_err(|error| {
        let mut failure = RecoveryFailure::journal(interrupted(error), &journal);
        if let Some(progress) = &mut failure.progress {
            progress.phase = entry_phase;
        }
        failure
    })
}

//! Internal completion and lease-free historical receipts. No host command is exposed.
use super::{flow::*, initialization::StagingMutationPort, journal::*};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::FileIdentity,
    state_recovery::{StoreRecovery, is_operation_id},
};

pub struct CompletionSnapshot {
    pub layout_two: bool,
    pub root_identity: FileIdentity,
    pub record: Option<Vec<u8>>,
    pub journal: Option<Vec<u8>>,
}
/// Only bounded marker/record/selected-journal reads. Never inspect current store data.
pub trait CompletionReadPort {
    fn read(&self) -> Result<CompletionSnapshot, RecoveryError>;
}
pub trait CompletionMutationPort {
    fn validate_locations(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn commit(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn write_receipt(&self, journal: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError>;
}
pub struct ReceiptSnapshot {
    pub layout_two: bool,
    pub root_identity: FileIdentity,
    pub backup_identity: Option<FileIdentity>,
    pub manifest: Option<Vec<u8>>,
    pub receipt: Option<Vec<u8>>,
}
/// Implemented without maintenance exclusion, SQLite or any current-store content.
pub trait ReceiptReadPort {
    fn read(&self, request: &RecoveryRequest<'_>) -> Result<ReceiptSnapshot, RecoveryError>;
}
pub struct CompletionPorts<'a> {
    pub read: &'a dyn CompletionReadPort,
    pub mutations: &'a dyn CompletionMutationPort,
    pub staging: &'a dyn StagingMutationPort,
}
fn request_binding(r: &RecoveryRequest<'_>, root: FileIdentity) -> Result<(), RecoveryError> {
    if !is_operation_id(r.operation_id)
        || r.proposal.operation_id != r.operation_id
        || r.proposal.engine_version != PRODUCT_VERSION
        || r.proposal.state_root != r.root.as_str()
        || r.proposal.root_identity != root
    {
        return Err(RecoveryError::StaleConfirmation);
    }
    Ok(())
}
/// Call before acquiring MaintenanceSession; a durable old receipt needs no lease.
pub fn completed_result(
    r: &RecoveryRequest<'_>,
    read: &dyn ReceiptReadPort,
    codec: &dyn JournalCodecPort,
) -> Result<Option<RecoveryResult>, RecoveryError> {
    let facts = read.read(r)?;
    if facts.layout_two {
        return Err(RecoveryError::UnsupportedLayout);
    }
    request_binding(r, facts.root_identity)?;
    let Some(receipt) = facts.receipt else {
        return Ok(None);
    };
    let manifest = facts.manifest.ok_or(RecoveryError::BackupConflict)?;
    let j = codec
        .decode(&manifest)
        .map_err(|_| RecoveryError::BackupConflict)?;
    let result = codec
        .decode_result(&receipt)
        .map_err(|_| RecoveryError::BackupConflict)?;
    if j.proposal != *r.proposal
        || j.backup_identity != facts.backup_identity
        || facts.backup_identity.is_none()
        || result.operation_id != r.operation_id
        || result.state_root != r.root.as_str()
        || result.backup_path != r.proposal.backup_path()
    {
        return Err(RecoveryError::BackupConflict);
    }
    Ok(Some(result))
}
/// Detect R8 before the archive observer, inventory or owner evidence can be read.
pub(super) fn committed(
    r: &RecoveryRequest<'_>,
    p: &RecoveryPorts<'_>,
    c: &CompletionPorts<'_>,
) -> Result<Option<RecoveryJournal>, RecoveryError> {
    let facts = c.read.read()?;
    if facts.layout_two {
        return Err(RecoveryError::UnsupportedLayout);
    }
    request_binding(r, facts.root_identity)?;
    if !facts
        .record
        .as_deref()
        .and_then(StoreRecovery::decode)
        .is_some_and(|v| v.commits(r.operation_id))
    {
        return Ok(None);
    }
    let bytes = facts.journal.ok_or(RecoveryError::RecoveryInterrupted)?;
    let j = p
        .journal_codec
        .decode(&bytes)
        .map_err(|_| RecoveryError::RecoveryInterrupted)?;
    if j.proposal != *r.proposal {
        return Err(RecoveryError::StaleConfirmation);
    }
    super::binding::verify_binding(
        p.session,
        r.proposal,
        r.root.as_str(),
        PRODUCT_VERSION,
        r.operation_id,
        super::binding::BindingMode::Resume {
            lock_identity: j.lock_identity,
        },
    )
    .map_err(|error| match error {
        super::binding::BindingError::Stale => RecoveryError::StaleConfirmation,
        super::binding::BindingError::ActiveOwner => RecoveryError::ActiveOwner,
        super::binding::BindingError::Unavailable => RecoveryError::InfrastructureUnavailable,
    })?;
    Ok(Some(j))
}
pub(super) fn initialize(
    mut j: RecoveryJournal,
    p: &RecoveryPorts<'_>,
    c: &CompletionPorts<'_>,
) -> Result<RecoveryJournal, RecoveryError> {
    c.staging.validate_archive(&j)?;
    // R7 may have stopped after the BackedUp journal but before its manifest.
    // Qualify original identities while allowing the R7b directory and reused names.
    let manifest = p
        .journal_codec
        .encode(&j)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    p.mutations.write_manifest(&j, &manifest)?;
    if j.staged.is_none() {
        c.staging.cleanup_staging(&j)?;
        j.staging = Some(c.staging.prepare_staging(&j)?);
        publish(&j, p)?;
        j.staged = Some(c.staging.initialize_staging(&j)?);
        publish(&j, p)?;
    }
    c.staging.publish_staged(&j)?;
    c.staging.validate_initialized(&j)?;
    c.mutations.commit(&j)?;
    Ok(j)
}
pub(super) fn finish(
    mut j: RecoveryJournal,
    p: &RecoveryPorts<'_>,
    c: &CompletionPorts<'_>,
) -> Result<RecoveryResult, RecoveryError> {
    // Once committed, current-store bytes and inventory are deliberately untouched.
    c.mutations.validate_locations(&j)?;
    let result = RecoveryResult {
        operation_id: j.operation_id.clone(),
        state_root: j.state_root.clone(),
        backup_path: j.backup_path.clone(),
    };
    j.result = Some(result.clone());
    j.phase = JournalPhase::Completed;
    let receipt = p
        .journal_codec
        .encode_result(&result)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    let bytes = p
        .journal_codec
        .encode(&j)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    c.mutations.write_receipt(&j, &receipt)?;
    p.mutations.write_manifest(&j, &bytes)?;
    p.mutations.write_journal(&j, &bytes)?;
    Ok(result)
}
fn publish(j: &RecoveryJournal, p: &RecoveryPorts<'_>) -> Result<(), RecoveryError> {
    let bytes = p
        .journal_codec
        .encode(j)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    p.mutations.write_journal(j, &bytes)
}

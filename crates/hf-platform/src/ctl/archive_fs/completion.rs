//! Postcommit reconciliation checks only stable directory/lock identities.
use super::{descriptor::*, *};
use hf_application::ctl::state_recover::{
    completion::CompletionMutationPort, journal::JOURNAL_BOUND,
};
use hf_domain::ctl::state_recovery::{RecoveryState, StoreRecovery};
use nix::fcntl::renameat;

impl CompletionMutationPort for FsArchive<'_> {
    fn validate_locations(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.dirs(j).map(|_| ())
    }
    fn commit(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let root = self.root()?;
        let record = StoreRecovery::new(&j.operation_id, RecoveryState::Committed)
            .ok_or(RecoveryError::BackupConflict)?;
        self.publish(
            j,
            &root,
            "store.recovery",
            "store.recovery.tmp",
            record.encode().as_bytes(),
            true,
        )
    }
    fn write_receipt(&self, j: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError> {
        let dirs = self.dirs(j)?;
        if let Some(saved) = read(&dirs.backup, "receipt.json.pending", JOURNAL_BOUND)? {
            self.qualify_receipt(j, &saved)?;
            self.flush_existing(j, &dirs.backup, "receipt.json.pending", &saved)?;
            self.replace_receipt(j, &dirs.backup)?;
        }
        if let Some(saved) = read(&dirs.backup, "receipt.json", JOURNAL_BOUND)? {
            self.qualify_receipt(j, &saved)?;
        }
        self.new_file(j, &dirs.backup, "receipt.json.pending", bytes)?;
        self.replace_receipt(j, &dirs.backup)
    }
}
impl FsArchive<'_> {
    fn qualify_receipt(&self, j: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError> {
        let result = self
            .codec
            .decode_result(bytes)
            .map_err(|_| RecoveryError::BackupConflict)?;
        if result.operation_id != j.operation_id
            || result.state_root != j.state_root
            || result.backup_path != j.backup_path
        {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(())
    }
    fn replace_receipt(&self, j: &RecoveryJournal, dir: &OwnedFd) -> Result<(), RecoveryError> {
        let source = read(dir, "receipt.json.pending", JOURNAL_BOUND)?
            .ok_or(RecoveryError::BackupConflict)?;
        self.qualify_receipt(j, &source)?;
        let target = read(dir, "receipt.json", JOURNAL_BOUND)?;
        if let Some(ref bytes) = target {
            self.qualify_receipt(j, bytes)?;
        }
        let source_id = stat(dir, "receipt.json.pending")?.as_ref().map(id);
        let target_id = stat(dir, "receipt.json")?.as_ref().map(id);
        self.boundary(j, "receipt.rename")?;
        if read(dir, "receipt.json.pending", JOURNAL_BOUND)?.as_deref() != Some(source.as_slice())
            || read(dir, "receipt.json", JOURNAL_BOUND)? != target
            || stat(dir, "receipt.json.pending")?.as_ref().map(id) != source_id
            || stat(dir, "receipt.json")?.as_ref().map(id) != target_id
        {
            return Err(RecoveryError::BackupConflict);
        }
        renameat(dir, "receipt.json.pending", dir, "receipt.json").map_err(failure)?;
        self.sync(j, dir, "receipt.directory_sync")
    }
}

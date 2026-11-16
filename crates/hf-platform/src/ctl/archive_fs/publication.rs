use super::{descriptor::*, *};
use hf_application::ctl::state_recover::journal::JOURNAL_BOUND;
use hf_application::durability::FlushPort;
use hf_domain::ctl::state_recovery::{RecoveryState, StoreRecovery};
use nix::{
    fcntl::{OFlag, openat, renameat},
    sys::stat::Mode,
};
use std::{fs::File, io::Write};

impl FsArchive<'_> {
    pub(super) fn pending(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let root = self.root()?;
        let record = StoreRecovery::new(&j.operation_id, RecoveryState::Pending)
            .ok_or(RecoveryError::StaleConfirmation)?;
        self.publish(
            j,
            &root,
            "store.recovery",
            "store.recovery.tmp",
            record.encode().as_bytes(),
            true,
        )
    }
    pub(super) fn publish(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        name: &str,
        temporary: &str,
        bytes: &[u8],
        record: bool,
    ) -> Result<(), RecoveryError> {
        if let Some(saved) = read(dir, temporary, JOURNAL_BOUND)? {
            self.qualify_temporary(j, &saved, record)?;
            self.flush_existing(j, dir, temporary, &saved)?;
            self.replace(j, dir, temporary, name, record)?;
        }
        self.new_file(j, dir, temporary, bytes)?;
        self.replace(j, dir, temporary, name, record)
    }
    fn qualify_temporary(
        &self,
        j: &RecoveryJournal,
        bytes: &[u8],
        record: bool,
    ) -> Result<(), RecoveryError> {
        if record {
            let saved = StoreRecovery::decode(bytes).ok_or(RecoveryError::BackupConflict)?;
            if saved.operation_id() != j.operation_id || !saved.is_pending() {
                return Err(RecoveryError::BackupConflict);
            }
        } else {
            let saved = self
                .codec
                .decode(bytes)
                .map_err(|_| RecoveryError::BackupConflict)?;
            if saved.proposal != j.proposal
                || saved.operation_id != j.operation_id
                || saved.root_identity != j.root_identity
                || saved.lock_identity != j.lock_identity
                || saved.inventory != j.inventory
                || saved.owner_evidence != j.owner_evidence
                || saved.previous_journal_bytes != j.previous_journal_bytes
                || saved
                    .backup_identity
                    .is_some_and(|id| Some(id) != j.backup_identity)
                || saved
                    .data_identity
                    .is_some_and(|id| Some(id) != j.data_identity)
            {
                return Err(RecoveryError::BackupConflict);
            }
        }
        Ok(())
    }
    fn replace(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        from: &str,
        to: &str,
        record: bool,
    ) -> Result<(), RecoveryError> {
        let source = read(dir, from, JOURNAL_BOUND)?.ok_or(RecoveryError::BackupConflict)?;
        let target = read(dir, to, JOURNAL_BOUND)?;
        let source_id = stat(dir, from)?.as_ref().map(id);
        let target_id = stat(dir, to)?.as_ref().map(id);
        self.qualify_temporary(j, &source, record)?;
        if let Some(bytes) = &target
            && !record
            && !(to == ".harflow-recovery.json"
                && j.previous_journal_bytes.as_deref() == Some(bytes.as_slice()))
        {
            self.qualify_temporary(j, bytes, false)?;
        }
        self.boundary(j, "publication.rename")?;
        if read(dir, from, JOURNAL_BOUND)?.as_deref() != Some(source.as_slice())
            || read(dir, to, JOURNAL_BOUND)? != target
            || stat(dir, from)?.as_ref().map(id) != source_id
            || stat(dir, to)?.as_ref().map(id) != target_id
        {
            return Err(RecoveryError::BackupConflict);
        }
        renameat(dir, from, dir, to).map_err(failure)?;
        self.sync(j, dir, "publication.directory_sync")
    }
    pub(super) fn flush_existing(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        name: &str,
        expected: &[u8],
    ) -> Result<(), RecoveryError> {
        let before = stat(dir, name)?.ok_or(RecoveryError::BackupConflict)?;
        if !regular(&before) {
            return Err(RecoveryError::BackupConflict);
        }
        self.boundary(j, "file.reflush")?;
        if read(dir, name, JOURNAL_BOUND)?.as_deref() != Some(expected) {
            return Err(RecoveryError::BackupConflict);
        }
        let opened = openat(
            dir,
            name,
            OFlag::O_RDWR | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(failure)?;
        if nix::sys::stat::fstat(&opened)
            .map_err(failure)
            .map(|s| id(&s))?
            != id(&before)
        {
            return Err(RecoveryError::BackupConflict);
        }
        crate::durability::FullFlush
            .flush(&File::from(opened))
            .map_err(failure)
    }
    pub(super) fn new_file(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), RecoveryError> {
        if bytes.len() > JOURNAL_BOUND {
            return Err(RecoveryError::InfrastructureUnavailable);
        }
        self.boundary(j, "file.create")?;
        let descriptor = openat(
            dir,
            name,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| {
            if e == nix::errno::Errno::EEXIST {
                RecoveryError::BackupConflict
            } else {
                failure(e)
            }
        })?;
        let mut file = File::from(descriptor);
        self.boundary(j, "file.write")?;
        file.write_all(bytes).map_err(failure)?;
        self.boundary(j, "file.flush")?;
        file.flush().map_err(failure)?;
        self.boundary(j, "file.sync")?;
        crate::durability::FullFlush.flush(&file).map_err(failure)
    }
}

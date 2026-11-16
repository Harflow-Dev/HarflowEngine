//! Staged initialization, preserving the original session and descriptor proofs.
mod files;
mod publish;
use super::{descriptor::*, *};
use hf_application::{
    ctl::state_recover::{
        initialization::{FreshStorePort, StagingMutationPort},
        journal::{JOURNAL_BOUND, StagedFile},
    },
    digest::DigestPort,
    durability::FlushPort,
};
use nix::{
    sys::stat::{Mode, mkdirat},
    unistd::{UnlinkatFlags, unlinkat},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const FILES: [&str; 4] = [
    "store.sqlite3",
    "store.sqlite3-wal",
    "store.watermark",
    "store.identity",
];
const PARTIAL_FILES: [&str; 5] = [
    "store.sqlite3",
    "store.sqlite3-wal",
    "store.sqlite3-shm",
    "store.watermark",
    "store.identity",
];

pub struct FsRecovery<'a> {
    archive: &'a FsArchive<'a>,
    initializer: &'a dyn FreshStorePort,
    digest: &'a dyn DigestPort,
    flush: &'a dyn FlushPort,
}
impl<'a> FsRecovery<'a> {
    pub fn new(
        archive: &'a FsArchive<'a>,
        initializer: &'a dyn FreshStorePort,
        digest: &'a dyn DigestPort,
        flush: &'a dyn FlushPort,
    ) -> Self {
        Self {
            archive,
            initializer,
            digest,
            flush,
        }
    }
    fn name(j: &RecoveryJournal) -> String {
        format!(".harflow-recovery-init-{}", j.operation_id)
    }
    fn staging(&self, j: &RecoveryJournal) -> Result<OwnedFd, RecoveryError> {
        let root = self.archive.root()?;
        let staging = directory(&root, &Self::name(j))?;
        if Some(identity(&staging)?) != j.staging {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(staging)
    }
    fn remove_directory(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let root = self.archive.root()?;
        self.archive.boundary(j, "staging.rmdir")?;
        if let Some(expected) = j.staging {
            let staging = self.staging(j)?;
            if identity(&staging)? != expected {
                return Err(RecoveryError::BackupConflict);
            }
        }
        unlinkat(&root, Self::name(j).as_str(), UnlinkatFlags::RemoveDir).map_err(failure)?;
        self.archive.sync(j, &root, "staging.remove_root_sync")
    }
}
impl StagingMutationPort for FsRecovery<'_> {
    fn validate_archive(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.archive_entries(j)
    }
    fn cleanup_staging(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let root = self.archive.root()?;
        let Some(_) = stat(&root, &Self::name(j))? else {
            return Ok(());
        };
        let staging = directory(&root, &Self::name(j))?;
        if j.staging.is_none() {
            if !names(&staging)?.is_empty() {
                return Err(RecoveryError::BackupConflict);
            }
            return self.remove_directory(j);
        }
        self.staging(j)?;
        let entries = names(&staging)?;
        // Validate the complete set before the first deletion.
        for name in &entries {
            if !PARTIAL_FILES.contains(&name.as_str())
                || !stat(&staging, name)?.as_ref().is_some_and(regular)
            {
                return Err(RecoveryError::BackupConflict);
            }
        }
        for name in entries {
            self.archive.boundary(j, "staging.cleanup_unlink")?;
            self.staging(j)?;
            for current in names(&staging)? {
                if !PARTIAL_FILES.contains(&current.as_str())
                    || !stat(&staging, &current)?.as_ref().is_some_and(regular)
                {
                    return Err(RecoveryError::BackupConflict);
                }
            }
            if !stat(&staging, &name)?.as_ref().is_some_and(regular) {
                return Err(RecoveryError::BackupConflict);
            }
            unlinkat(&staging, name.as_str(), UnlinkatFlags::NoRemoveDir).map_err(failure)?;
        }
        self.remove_directory(j)
    }
    fn prepare_staging(&self, j: &RecoveryJournal) -> Result<FileIdentity, RecoveryError> {
        let root = self.archive.root()?;
        absent(&root, &Self::name(j))?;
        self.archive.boundary(j, "staging.mkdir")?;
        mkdirat(
            &root,
            Self::name(j).as_str(),
            Mode::from_bits_truncate(0o700),
        )
        .map_err(failure)?;
        let staging = directory(&root, &Self::name(j))?;
        self.archive.sync(j, &root, "staging.create_root_sync")?;
        identity(&staging)
    }
    fn initialize_staging(
        &self,
        j: &RecoveryJournal,
    ) -> Result<BTreeMap<String, StagedFile>, RecoveryError> {
        let staging = self.staging(j)?;
        if !names(&staging)?.is_empty() {
            return Err(RecoveryError::BackupConflict);
        }
        self.archive.boundary(j, "staging.initialize")?;
        self.staging(j)?;
        if !names(&staging)?.is_empty() {
            return Err(RecoveryError::BackupConflict);
        }
        self.initializer.initialize(
            &std::path::Path::new(&j.state_root).join(Self::name(j)),
            j.staging.ok_or(RecoveryError::BackupConflict)?,
            Instant::now() + Duration::from_secs(10),
        )?;
        self.record_files(j)
    }
    fn publish_staged(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.publish_files(j)
    }
    fn validate_initialized(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.initialized(j)
    }
}

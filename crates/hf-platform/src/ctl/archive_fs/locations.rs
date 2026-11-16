use super::{descriptor::*, *};
use crate::ctl::inventory::descriptor::entry_at;
use hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED;
use std::ffi::OsStr;

impl FsArchive<'_> {
    pub(super) fn locations(
        &self,
        j: &RecoveryJournal,
        final_only: bool,
    ) -> Result<(), RecoveryError> {
        let dirs = self.dirs(j)?;
        for name in names(&dirs.root)? {
            if INVENTORY_EXCLUDED.contains(&name.as_str()) {
                let s = stat(&dirs.root, &name)?.ok_or(RecoveryError::StaleConfirmation)?;
                if !regular(&s) {
                    return Err(RecoveryError::UnsafeRoot);
                }
            } else if !j.inventory.entries.contains_key(&name) {
                return Err(RecoveryError::StaleConfirmation);
            }
        }
        if names(&dirs.data)?
            .iter()
            .any(|n| n != "store.sqlite3-shm" && !j.inventory.entries.contains_key(n))
        {
            return Err(RecoveryError::StaleConfirmation);
        }
        for at in [&dirs.root, &dirs.data] {
            if let Some(s) = stat(at, "store.sqlite3-shm")?
                && !regular(&s)
            {
                return Err(RecoveryError::UnsafeRoot);
            }
        }
        if final_only && stat(&dirs.root, "store.sqlite3-shm")?.is_some() {
            return Err(RecoveryError::BackupConflict);
        }
        for (name, expected) in &j.inventory.entries {
            let source = stat(&dirs.root, name)?.is_some();
            let target = stat(&dirs.data, name)?.is_some();
            if source == target || (final_only && source) {
                return Err(RecoveryError::BackupConflict);
            }
            let at = if source { &dirs.root } else { &dirs.data };
            if entry_at(at, OsStr::new(name)).map_err(|_| RecoveryError::StaleConfirmation)?
                != *expected
            {
                return Err(RecoveryError::StaleConfirmation);
            }
        }
        self.dirs(j)?;
        Ok(())
    }
    pub(super) fn move_one(
        &self,
        j: &RecoveryJournal,
        name: &str,
        expected: &InventoryEntry,
    ) -> Result<(), RecoveryError> {
        let dirs = self.dirs(j)?;
        let source = stat(&dirs.root, name)?.is_some();
        let target = stat(&dirs.data, name)?.is_some();
        if source == target {
            return Err(RecoveryError::BackupConflict);
        }
        let at = if source { &dirs.root } else { &dirs.data };
        if entry_at(at, OsStr::new(name)).map_err(|_| RecoveryError::StaleConfirmation)?
            != *expected
        {
            return Err(RecoveryError::StaleConfirmation);
        }
        if source {
            self.boundary(j, "entry.rename")?;
            // Recompare after the injectable boundary, immediately before mutation.
            if entry_at(&dirs.root, OsStr::new(name))
                .map_err(|_| RecoveryError::StaleConfirmation)?
                != *expected
            {
                return Err(RecoveryError::StaleConfirmation);
            }
            self.rename
                .rename_exclusive(&dirs.root, name, &dirs.data, name)
                .map_err(rename_error)?;
        }
        // Sync both sides even if a previous attempt renamed but failed its sync.
        self.sync(j, &dirs.root, "entry.root_sync")?;
        self.sync(j, &dirs.data, "entry.data_sync")
    }
    pub(super) fn shm(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let dirs = self.dirs(j)?;
        let name = "store.sqlite3-shm";
        let source = stat(&dirs.root, name)?;
        let target = stat(&dirs.data, name)?;
        if source.is_some() && target.is_some() {
            return Err(RecoveryError::BackupConflict);
        }
        if source.as_ref().is_some_and(|s| !regular(s))
            || target.as_ref().is_some_and(|s| !regular(s))
        {
            return Err(RecoveryError::UnsafeRoot);
        }
        if let Some(before) = source {
            self.boundary(j, "shm.rename")?;
            if stat(&dirs.root, name)?.as_ref().map(id) != Some(id(&before)) {
                return Err(RecoveryError::StaleConfirmation);
            }
            self.rename
                .rename_exclusive(&dirs.root, name, &dirs.data, name)
                .map_err(rename_error)?;
        }
        self.sync(j, &dirs.root, "shm.root_sync")?;
        self.sync(j, &dirs.data, "shm.data_sync")
    }
}
fn rename_error(e: io::Error) -> RecoveryError {
    if e.kind() == io::ErrorKind::AlreadyExists {
        RecoveryError::BackupConflict
    } else {
        RecoveryError::InfrastructureUnavailable
    }
}

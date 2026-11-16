use super::*;
use hf_application::watermark;

impl FsRecovery<'_> {
    pub(super) fn publish_files(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.archive_entries(j)?;
        let dirs = self.archive.dirs(j)?;
        let staging = if stat(&dirs.root, &Self::name(j))?.is_some() {
            Some(self.staging(j)?)
        } else {
            None
        };
        let staged = j.staged.as_ref().ok_or(RecoveryError::BackupConflict)?;
        // Qualify the complete prefix before any move: zero/both locations refuse.
        for name in FILES {
            self.location(j, staging.as_ref(), &dirs.root, name)?;
        }
        if let Some(ref staging) = staging
            && names(staging)?
                .iter()
                .any(|name| !staged.contains_key(name))
        {
            return Err(RecoveryError::BackupConflict);
        }
        for name in FILES {
            if self.location(j, staging.as_ref(), &dirs.root, name)? {
                let source = staging.as_ref().ok_or(RecoveryError::BackupConflict)?;
                self.archive.boundary(j, "staging.rename")?;
                self.staging(j)?;
                self.location(j, Some(source), &dirs.root, name)?;
                self.archive
                    .rename
                    .rename_exclusive(source, name, &dirs.root, name)
                    .map_err(|e| {
                        if e.kind() == io::ErrorKind::AlreadyExists {
                            RecoveryError::BackupConflict
                        } else {
                            failure(e)
                        }
                    })?;
            }
            if let Some(ref staging) = staging {
                self.archive
                    .sync(j, staging, "staging.rename_source_sync")?;
            }
            self.archive
                .sync(j, &dirs.root, "staging.rename_root_sync")?;
        }
        if let Some(staging) = staging {
            if !names(&staging)?.is_empty() {
                return Err(RecoveryError::BackupConflict);
            }
            self.remove_directory(j)?;
        }
        self.initialized(j)
    }
    /// True iff the exact staged inode is still in staging.
    fn location(
        &self,
        j: &RecoveryJournal,
        staging: Option<&OwnedFd>,
        root: &OwnedFd,
        name: &str,
    ) -> Result<bool, RecoveryError> {
        let expected = j
            .staged
            .as_ref()
            .and_then(|files| files.get(name))
            .ok_or(RecoveryError::BackupConflict)?;
        let source = staging
            .map(|dir| stat(dir, name))
            .transpose()?
            .flatten()
            .is_some();
        let target = stat(root, name)?.is_some();
        if source == target {
            return Err(RecoveryError::BackupConflict);
        }
        let at = if source {
            staging.ok_or(RecoveryError::BackupConflict)?
        } else {
            root
        };
        if self.file(at, name)? != *expected {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(source)
    }
    pub(super) fn initialized(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.archive_entries(j)?;
        let root = self.archive.root()?;
        for name in FILES {
            if self.location(j, None, &root, name)? {
                return Err(RecoveryError::BackupConflict);
            }
        }
        if stat(&root, &Self::name(j))?.is_some() {
            return Err(RecoveryError::BackupConflict);
        }
        let wal = stat(&root, "store.sqlite3-wal")?.ok_or(RecoveryError::BackupConflict)?;
        if !regular(&wal) || wal.st_size == 0 {
            return Err(RecoveryError::BackupConflict);
        }
        let marker = read(&root, "store.identity", 128)?.ok_or(RecoveryError::BackupConflict)?;
        let text = std::str::from_utf8(&marker).map_err(failure)?;
        let lines: Vec<_> = text.lines().collect();
        if lines.len() != 3 || lines[0] != "harflow-storage-2" || lines[2].len() != 32 {
            return Err(RecoveryError::BackupConflict);
        }
        let bytes = read(&root, "store.watermark", watermark::FILE_LEN)?
            .ok_or(RecoveryError::BackupConflict)?;
        if !matches!(
            watermark::decode(&bytes, lines[2]),
            watermark::Decoded::Valid { seq: 0, .. }
        ) {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(())
    }
}

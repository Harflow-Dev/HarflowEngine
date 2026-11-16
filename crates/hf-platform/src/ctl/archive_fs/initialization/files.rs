use super::*;
use hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED;
use nix::{
    fcntl::{OFlag, openat},
    sys::stat::{Mode, fstat},
};
use std::fs::File;

impl FsRecovery<'_> {
    pub(super) fn archive_entries(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        let dirs = self.archive.dirs(j)?;
        let originals = original_identities(&dirs.data, &j.inventory.entries)?;
        if names(&dirs.data)?
            .iter()
            .any(|name| name != "store.sqlite3-shm" && !j.inventory.entries.contains_key(name))
        {
            return Err(RecoveryError::BackupConflict);
        }
        for name in names(&dirs.root)? {
            let s = stat(&dirs.root, &name)?.ok_or(RecoveryError::BackupConflict)?;
            if originals.contains(&(id(&s).dev, id(&s).ino)) {
                return Err(RecoveryError::BackupConflict);
            }
            if INVENTORY_EXCLUDED.contains(&name.as_str())
                && name != "store.sqlite3-shm"
                && regular(&s)
            {
                continue;
            }
            if name == Self::name(j) {
                let staging = directory(&dirs.root, &name)?;
                if j.staging
                    .is_some_and(|expected| identity(&staging).ok() != Some(expected))
                {
                    return Err(RecoveryError::BackupConflict);
                }
                continue;
            }
            if !j
                .staged
                .as_ref()
                .and_then(|files| files.get(&name))
                .is_some_and(|saved| saved.identity == id(&s) && regular(&s))
            {
                return Err(RecoveryError::BackupConflict);
            }
        }
        self.archive.dirs(j)?;
        Ok(())
    }
    pub(super) fn record_files(
        &self,
        j: &RecoveryJournal,
    ) -> Result<BTreeMap<String, StagedFile>, RecoveryError> {
        let staging = self.staging(j)?;
        for name in names(&staging)? {
            if !PARTIAL_FILES.contains(&name.as_str())
                || !stat(&staging, &name)?.as_ref().is_some_and(regular)
            {
                return Err(RecoveryError::BackupConflict);
            }
        }
        if stat(&staging, "store.sqlite3-shm")?.is_some() {
            self.archive.boundary(j, "staging.shm_unlink")?;
            self.staging(j)?;
            if !stat(&staging, "store.sqlite3-shm")?
                .as_ref()
                .is_some_and(regular)
            {
                return Err(RecoveryError::BackupConflict);
            }
            unlinkat(&staging, "store.sqlite3-shm", UnlinkatFlags::NoRemoveDir).map_err(failure)?;
        }
        for name in ["store.sqlite3", "store.sqlite3-wal"] {
            self.flush_file(j, &staging, name)?;
        }
        self.archive.sync(j, &staging, "staging.initialized_sync")?;
        let mut result = BTreeMap::new();
        if names(&staging)?.len() != FILES.len() {
            return Err(RecoveryError::BackupConflict);
        }
        for name in FILES {
            result.insert(name.into(), self.file(&staging, name)?);
        }
        self.staging(j)?;
        Ok(result)
    }
    pub(super) fn file(&self, dir: &OwnedFd, name: &str) -> Result<StagedFile, RecoveryError> {
        let before = stat(dir, name)?.ok_or(RecoveryError::BackupConflict)?;
        if !regular(&before) || before.st_size < 0 {
            return Err(RecoveryError::BackupConflict);
        }
        let bytes = read(dir, name, JOURNAL_BOUND)?.ok_or(RecoveryError::BackupConflict)?;
        let after = stat(dir, name)?.ok_or(RecoveryError::BackupConflict)?;
        if id(&before) != id(&after)
            || before.st_size != after.st_size
            || mtime(&before)? != mtime(&after)?
        {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(StagedFile {
            identity: id(&before),
            size: before.st_size as u64,
            mtime_ns: mtime(&before)?,
            sha256: self.digest.digest_parts(&[&bytes]).as_str().into(),
        })
    }
    fn flush_file(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        name: &str,
    ) -> Result<(), RecoveryError> {
        let before = stat(dir, name)?.ok_or(RecoveryError::BackupConflict)?;
        if !regular(&before) {
            return Err(RecoveryError::BackupConflict);
        }
        self.archive.boundary(j, "staging.file_flush")?;
        self.staging(j)?;
        let fd = openat(
            dir,
            name,
            OFlag::O_RDWR | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(failure)?;
        if id(&fstat(&fd).map_err(failure)?) != id(&before) {
            return Err(RecoveryError::BackupConflict);
        }
        self.flush.flush(&File::from(fd)).map_err(failure)
    }
}
fn mtime(s: &nix::sys::stat::FileStat) -> Result<i64, RecoveryError> {
    s.st_mtime
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(s.st_mtime_nsec))
        .ok_or(RecoveryError::BackupConflict)
}

/// Recheck every recorded original identity, including descendants, without re-reading
/// archived contents or requiring unchanged attributes after R7.
fn original_identities(
    data: &OwnedFd,
    inventory: &BTreeMap<String, hf_domain::ctl::recovery_proposal::InventoryEntry>,
) -> Result<std::collections::BTreeSet<(i64, u64)>, RecoveryError> {
    use hf_domain::ctl::recovery_proposal::EntryBody;
    let mut ids = std::collections::BTreeSet::new();
    let mut stack = vec![(nix::unistd::dup(data).map_err(failure)?, inventory.iter())];
    while let Some((dir, mut entries)) = stack.pop() {
        let Some((name, entry)) = entries.next() else {
            continue;
        };
        let saved = stat(&dir, name)?.ok_or(RecoveryError::BackupConflict)?;
        let expected = FileIdentity {
            dev: entry.stat.dev,
            ino: entry.stat.ino,
        };
        if id(&saved) != expected {
            return Err(RecoveryError::BackupConflict);
        }
        ids.insert((expected.dev, expected.ino));
        if let EntryBody::Directory(children) = &entry.body {
            let child = directory(&dir, name)?;
            if identity(&child)? != expected {
                return Err(RecoveryError::BackupConflict);
            }
            stack.push((dir, entries));
            stack.push((child, children.iter()));
        } else {
            stack.push((dir, entries));
        }
    }
    Ok(ids)
}

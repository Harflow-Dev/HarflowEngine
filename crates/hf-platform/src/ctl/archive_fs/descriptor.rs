use super::*;
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, open, openat},
    libc,
    sys::stat::{FileStat, Mode, fstat, fstatat},
};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};

pub(super) fn failure<T>(_: T) -> RecoveryError {
    RecoveryError::InfrastructureUnavailable
}
pub(super) fn flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}
pub(super) fn directory(parent: &OwnedFd, name: &str) -> Result<OwnedFd, RecoveryError> {
    openat(parent, name, flags(), Mode::empty()).map_err(|_| RecoveryError::BackupConflict)
}
pub(super) fn absolute(path: &Path) -> Result<OwnedFd, RecoveryError> {
    if !path.is_absolute() {
        return Err(RecoveryError::UnsafeRoot);
    }
    let mut fd = open("/", flags(), Mode::empty()).map_err(failure)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                fd = openat(&fd, name, flags(), Mode::empty())
                    .map_err(|_| RecoveryError::UnsafeRoot)?
            }
            _ => return Err(RecoveryError::UnsafeRoot),
        }
    }
    Ok(fd)
}
pub(super) fn parent(path: &str) -> Result<(OwnedFd, String), RecoveryError> {
    let path = Path::new(path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(RecoveryError::UnsafeRoot)?;
    let parent = path.parent().ok_or(RecoveryError::UnsafeRoot)?;
    Ok((absolute(parent)?, name.into()))
}
pub(super) fn identity(fd: &OwnedFd) -> Result<FileIdentity, RecoveryError> {
    fstat(fd).map(|s| id(&s)).map_err(failure)
}
pub(super) fn id(s: &FileStat) -> FileIdentity {
    FileIdentity {
        dev: i64::from(s.st_dev),
        ino: s.st_ino,
    }
}
pub(super) fn stat(fd: &OwnedFd, name: &str) -> Result<Option<FileStat>, RecoveryError> {
    match fstatat(fd, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(s) => Ok(Some(s)),
        Err(Errno::ENOENT) => Ok(None),
        Err(e) => Err(failure(e)),
    }
}
pub(super) fn regular(s: &FileStat) -> bool {
    u32::from(s.st_mode) & u32::from(libc::S_IFMT) == u32::from(libc::S_IFREG)
}
pub(super) fn absent(fd: &OwnedFd, name: &str) -> Result<(), RecoveryError> {
    if stat(fd, name)?.is_some() {
        Err(RecoveryError::BackupConflict)
    } else {
        Ok(())
    }
}
pub(super) fn names(fd: &OwnedFd) -> Result<Vec<String>, RecoveryError> {
    crate::ctl::inventory::descriptor::names_at(fd)
        .map_err(failure)?
        .into_iter()
        .map(|n| n.into_string().map_err(|_| RecoveryError::UnsafeRoot))
        .collect()
}
pub(super) fn read(
    fd: &OwnedFd,
    name: &str,
    bound: usize,
) -> Result<Option<Vec<u8>>, RecoveryError> {
    let Some(before) = stat(fd, name)? else {
        return Ok(None);
    };
    if !regular(&before) || before.st_size < 0 || before.st_size as u64 > bound as u64 {
        return Err(RecoveryError::BackupConflict);
    }
    let opened = openat(
        fd,
        name,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(failure)?;
    let after = fstat(&opened).map_err(failure)?;
    if id(&before) != id(&after) || !regular(&after) {
        return Err(RecoveryError::BackupConflict);
    }
    let mut bytes = Vec::new();
    File::from(opened)
        .take(bound as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > bound {
        return Err(RecoveryError::BackupConflict);
    }
    Ok(Some(bytes))
}
pub(super) struct Dirs {
    pub root: OwnedFd,
    pub backup: OwnedFd,
    pub data: OwnedFd,
}
impl FsArchive<'_> {
    pub(super) fn root(&self) -> Result<OwnedFd, RecoveryError> {
        self.session
            .verify_held()
            .map_err(|_| RecoveryError::StaleConfirmation)?;
        let root = absolute(Path::new(self.session.state_root()))?;
        if identity(&root)? != self.session.held_root_identity() {
            return Err(RecoveryError::StaleConfirmation);
        }
        Ok(root)
    }
    pub(super) fn dirs(&self, j: &RecoveryJournal) -> Result<Dirs, RecoveryError> {
        let root = self.root()?;
        let (parent, name) = parent(&j.backup_path)?;
        let backup = directory(&parent, &name)?;
        let data = directory(&backup, "data")?;
        if Some(identity(&backup)?) != j.backup_identity
            || Some(identity(&data)?) != j.data_identity
        {
            return Err(RecoveryError::BackupConflict);
        }
        Ok(Dirs { root, backup, data })
    }
    pub(super) fn boundary(
        &self,
        j: &RecoveryJournal,
        name: &'static str,
    ) -> Result<(), RecoveryError> {
        self.faults.before(name).map_err(failure)?;
        if j.backup_identity.is_some() && j.data_identity.is_some() {
            self.dirs(j)?;
        } else {
            self.root()?;
            if let Some(expected) = j.backup_identity {
                let (parent, name) = parent(&j.backup_path)?;
                let backup = directory(&parent, &name)?;
                if identity(&backup)? != expected {
                    return Err(RecoveryError::BackupConflict);
                }
            }
        }
        Ok(())
    }
    pub(super) fn sync(
        &self,
        j: &RecoveryJournal,
        dir: &OwnedFd,
        label: &'static str,
    ) -> Result<(), RecoveryError> {
        self.boundary(j, label)?;
        match nix::unistd::fsync(dir) {
            Ok(()) | Err(Errno::EINVAL) => Ok(()),
            Err(e) => Err(failure(e)),
        }
    }
}

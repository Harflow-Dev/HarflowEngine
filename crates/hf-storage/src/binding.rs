use crate::{Error, Result};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{self, Metadata},
    path::{Path, PathBuf},
};
#[derive(Clone, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}
fn identify(path: &Path, directory: bool) -> Result<FileIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(Error::InvalidRoot);
    }
    identity(&metadata)
}
#[cfg(unix)]
fn identity(metadata: &Metadata) -> Result<FileIdentity> {
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
#[cfg(not(unix))]
fn identity(_: &Metadata) -> Result<FileIdentity> {
    Err(Error::InvalidRoot)
}
pub(crate) struct Binding {
    root: PathBuf,
    root_id: FileIdentity,
    database_id: Option<FileIdentity>,
    lock_id: FileIdentity,
}
impl Binding {
    pub(crate) fn validate_probe_path(root: &Path) -> Result<()> {
        let absolute = std::path::absolute(root)?;
        let mut prefix = PathBuf::new();
        for component in absolute.components() {
            prefix.push(component);
            match fs::symlink_metadata(&prefix) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(Error::InvalidRoot);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    pub(crate) fn validate_parent(root: &Path) -> Result<()> {
        let absolute = std::path::absolute(root)?;
        let parent = absolute.parent().ok_or(Error::InvalidRoot)?;
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            prefix.push(component);
            if fs::symlink_metadata(&prefix)?.file_type().is_symlink() {
                return Err(Error::InvalidRoot);
            }
        }
        Ok(())
    }
    pub(crate) fn capture(root: &Path, existing: bool) -> Result<Self> {
        // Reject symlinks in every supplied ancestor, including relative paths.
        let absolute = std::path::absolute(root)?;
        let mut prefix = PathBuf::new();
        for component in absolute.components() {
            prefix.push(component);
            if fs::symlink_metadata(&prefix)?.file_type().is_symlink() {
                return Err(Error::InvalidRoot);
            }
        }
        let root_id = identify(&absolute, true)?;
        let lock_id = identify(&absolute.join("store.lock"), false)?;
        let database_id = if existing {
            Some(identify(&absolute.join("store.sqlite3"), false)?)
        } else {
            None
        };
        let binding = Self {
            root: absolute,
            root_id,
            database_id,
            lock_id,
        };
        if existing {
            binding.check()?;
        }
        Ok(binding)
    }
    pub(crate) fn database(&self) -> PathBuf {
        self.root.join("store.sqlite3")
    }
    pub(crate) fn check_root(&self) -> Result<()> {
        if identify(&self.root, true)? != self.root_id {
            return Err(Error::Replaced);
        }
        Ok(())
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.check_root()?;
        if identify(&self.root.join("store.lock"), false)? != self.lock_id {
            return Err(Error::Replaced);
        }
        if self.database_id.as_ref() != Some(&identify(&self.database(), false)?) {
            return Err(Error::Replaced);
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = self.root.join(format!("store.sqlite3{suffix}"));
            match fs::symlink_metadata(path) {
                Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                    return Err(Error::InvalidRoot);
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

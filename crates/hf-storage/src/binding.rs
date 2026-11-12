use crate::{Error, Result};
use hf_application::lease::{LeaseGuard, LeaseMode, LeasePort};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{self, Metadata},
    path::{Path, PathBuf},
};
use std::{sync::Arc, time::Instant};
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
fn protocol_identity(path: &Path) -> Result<FileIdentity> {
    identify(path, false).map_err(|error| match error {
        Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound => Error::Incompatible,
        other => other,
    })
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
    leases: Arc<dyn LeasePort>,
    root: PathBuf,
    root_id: FileIdentity,
    database_id: Option<FileIdentity>,
    lock_id: FileIdentity,
    io_id: FileIdentity,
    marker_id: Option<FileIdentity>,
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
    pub(crate) fn capture(root: &Path, existing: bool, leases: Arc<dyn LeasePort>) -> Result<Self> {
        let binding = Self::identify_all(root, existing, leases)?;
        if existing {
            // The `-wal` type is left to `qualify::precheck`, which reports it as `WalMissing`.
            binding.check_sidecars(false)?;
            crate::wal::marker(&binding)?;
            crate::connection::qualify_header(&binding.database())?;
        }
        Ok(binding)
    }
    /// Identities of an existing root only: no sidecar, marker or header check.
    pub(crate) fn identify_existing(root: &Path, leases: Arc<dyn LeasePort>) -> Result<Self> {
        Self::identify_all(root, true, leases)
    }
    fn identify_all(root: &Path, existing: bool, leases: Arc<dyn LeasePort>) -> Result<Self> {
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
        Ok(Self {
            leases,
            root: absolute.clone(),
            root_id,
            database_id,
            lock_id,
            io_id: protocol_identity(&absolute.join("io.lock"))?,
            marker_id: if existing {
                Some(protocol_identity(&absolute.join("store.identity"))?)
            } else {
                None
            },
        })
    }
    pub(crate) fn source_stamp(
        &self,
        generation: hf_domain::ownership::Id,
    ) -> Result<hf_domain::recovery::SourceStamp> {
        let convert = |id: &FileIdentity| hf_domain::capture::FileIdentity {
            device: id.device,
            inode: id.inode,
        };
        self.check()?;
        Ok(hf_domain::recovery::SourceStamp {
            generation,
            root: convert(&self.root_id),
            database: convert(self.database_id.as_ref().ok_or(Error::Replaced)?),
            marker: convert(self.marker_id.as_ref().ok_or(Error::Replaced)?),
            store_lock: convert(&self.lock_id),
            io_lock: convert(&self.io_id),
        })
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn acquire(
        &self,
        mode: LeaseMode,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>> {
        self.leases
            .acquire(&self.root.join("store.lock"), mode, false, deadline)
            .map_err(crate::lease_error)
    }
    pub(crate) fn claim(&self, mode: LeaseMode, deadline: Instant) -> Result<Box<dyn LeaseGuard>> {
        let guard = self.acquire(mode, deadline)?;
        self.check_sidecars(false)?;
        crate::wal::marker(self)?;
        crate::connection::qualify_header(&self.database())?;
        Ok(guard)
    }
    pub(crate) fn io(&self, deadline: Instant) -> Result<Box<dyn LeaseGuard>> {
        let guard = self
            .leases
            .acquire(
                &self.root.join("io.lock"),
                LeaseMode::Exclusive,
                false,
                deadline,
            )
            .map_err(crate::lease_error)?;
        self.check()?;
        Ok(guard)
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
        self.check_sidecars(true)
    }
    /// Without `wal`, the `-wal` sidecar is left to the caller.
    pub(crate) fn check_sidecars(&self, wal: bool) -> Result<()> {
        self.check_root()?;
        if identify(&self.root.join("store.lock"), false)? != self.lock_id {
            return Err(Error::Replaced);
        }
        if identify(&self.root.join("io.lock"), false)? != self.io_id {
            return Err(Error::Replaced);
        }
        if let Some(marker) = &self.marker_id
            && &identify(&self.root.join("store.identity"), false)? != marker
        {
            return Err(Error::Replaced);
        }
        if self.database_id.as_ref() != Some(&identify(&self.database(), false)?) {
            return Err(Error::Replaced);
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            if suffix == "-wal" && !wal {
                continue;
            }
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

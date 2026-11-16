//! A bounded, no-follow inventory of a state root, for recovery proposals.
//!
//! On macOS the walk runs on directory descriptors: the root is opened without following
//! a link, every directory is opened relative to its parent without following a link, and
//! every entry is classified with `fstatat(AT_SYMLINK_NOFOLLOW)`, so a path swapped
//! during the walk cannot redirect it. Symbolic links are recorded with their target and
//! never followed; a socket, FIFO or device is `Special`, and so is any top-level name in
//! `INVENTORY_EXCLUDED` that is not a regular file (the name is otherwise skipped). Each entry's attributes are read
//! before and after it is visited, and a difference, or a vanished entry, is `Changed`.
//! The walk reads no file, opens no database and takes no lease. Other targets cannot
//! walk a root (`Unsupported`): recovery is store-gated to macOS.
use hf_application::ctl::state_inspect::InventoryPort;
use hf_domain::ctl::{
    recovery_proposal::{Inventory, InventoryFailure},
    state_root::StateRoot,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct FsInventory;

impl InventoryPort for FsInventory {
    fn walk(&self, root: &StateRoot, bound: usize) -> Result<Inventory, InventoryFailure> {
        #[cfg(target_os = "macos")]
        {
            descriptor::walk(std::path::Path::new(root.as_str()), bound)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (root, bound);
            Err(InventoryFailure::Unsupported)
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod descriptor {
    use hf_domain::ctl::recovery_proposal::{
        EntryBody, EntryStat, FileIdentity, INVENTORY_EXCLUDED, Inventory, InventoryEntry,
        InventoryFailure,
    };
    use nix::{
        dir::Dir,
        errno::Errno,
        fcntl::{AtFlags, OFlag, open, openat, readlinkat},
        libc,
        sys::stat::{FileStat, Mode, fstat, fstatat},
    };
    use std::{
        collections::BTreeMap,
        ffi::{OsStr, OsString},
        os::{fd::OwnedFd, unix::ffi::OsStrExt},
        path::Path,
    };

    /// Directories nested deeper than this are not walked (each level holds a descriptor).
    const MAX_DEPTH: usize = 128;
    // Bind recovery inventory to the lock identity used by the Rust store.
    const LOCK_NAME: &str = "store.lock";

    type Failure = InventoryFailure;

    fn flags(extra: OFlag) -> OFlag {
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC | extra
    }

    fn kind(stat: &FileStat) -> u32 {
        u32::from(stat.st_mode) & u32::from(libc::S_IFMT)
    }

    fn attributes(stat: &FileStat) -> Result<EntryStat, Failure> {
        let mtime_ns = stat
            .st_mtime
            .checked_mul(1_000_000_000)
            .and_then(|seconds| seconds.checked_add(stat.st_mtime_nsec))
            .ok_or(Failure::Unreadable)?;
        Ok(EntryStat {
            dev: i64::from(stat.st_dev),
            ino: stat.st_ino,
            mode: u32::from(stat.st_mode),
            size: stat.st_size,
            mtime_ns,
        })
    }

    /// A vanished entry changed under the walk; anything else cannot be read.
    fn failure(errno: Errno) -> Failure {
        match errno {
            Errno::ENOENT | Errno::ELOOP | Errno::ENOTDIR => Failure::Changed,
            _ => Failure::Unreadable,
        }
    }

    fn stat_at(dir: &OwnedFd, name: &OsStr) -> Result<FileStat, Failure> {
        fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(failure)
    }

    fn lock_identity(root: &OwnedFd) -> Result<Option<FileIdentity>, Failure> {
        match fstatat(root, LOCK_NAME, AtFlags::AT_SYMLINK_NOFOLLOW) {
            Err(Errno::ENOENT) => Ok(None),
            Err(_) => Err(Failure::Unreadable),
            Ok(stat) if kind(&stat) == u32::from(libc::S_IFREG) => Ok(Some(FileIdentity {
                dev: i64::from(stat.st_dev),
                ino: stat.st_ino,
            })),
            Ok(_) => Err(Failure::Unreadable),
        }
    }

    pub(super) fn walk(path: &Path, bound: usize) -> Result<Inventory, Failure> {
        let root = open(path, flags(OFlag::O_DIRECTORY), Mode::empty()).map_err(failure)?;
        let opened = fstat(&root).map_err(failure)?;
        let lock = lock_identity(&root)?;
        let mut walker = Walker { budget: 0, bound };
        let entries = walker.children(&root, 0, true)?;
        // The path still names the directory that was walked, with the same lock.
        let named = std::fs::symlink_metadata(path).map_err(|_| Failure::Changed)?;
        let unchanged = {
            use std::os::unix::fs::MetadataExt;
            named.ino() == opened.st_ino && named.dev() == opened.st_dev as u64
        };
        // The root's own attributes change when a top-level entry is added or removed.
        if !unchanged
            || lock_identity(&root)? != lock
            || attributes(&fstat(&root).map_err(failure)?)? != attributes(&opened)?
        {
            return Err(Failure::Changed);
        }
        Ok(Inventory {
            root: FileIdentity {
                dev: i64::from(opened.st_dev),
                ino: opened.st_ino,
            },
            lock,
            entries,
        })
    }

    pub(crate) fn entry_at(dir: &OwnedFd, name: &OsStr) -> Result<InventoryEntry, Failure> {
        Walker {
            budget: 0,
            bound: hf_domain::ctl::recovery_proposal::INVENTORY_BOUND,
        }
        .entry(dir, name, 0)
    }
    pub(crate) fn names_at(dir: &OwnedFd) -> Result<Vec<OsString>, Failure> {
        Walker {
            budget: 0,
            bound: hf_domain::ctl::recovery_proposal::INVENTORY_BOUND,
        }
        .names(dir)
    }

    struct Walker {
        budget: usize,
        bound: usize,
    }
    impl Walker {
        fn count(&mut self) -> Result<(), Failure> {
            self.budget += 1;
            if self.budget > self.bound {
                return Err(Failure::TooLarge);
            }
            Ok(())
        }

        /// Names in `dir`, listed from a duplicate descriptor of the pinned directory.
        fn names(&self, dir: &OwnedFd) -> Result<Vec<OsString>, Failure> {
            let copy = dir.try_clone().map_err(|_| Failure::Unreadable)?;
            let mut listing = Dir::from_fd(copy).map_err(|_| Failure::Unreadable)?;
            let mut names = Vec::new();
            for item in listing.iter() {
                let item = item.map_err(|_| Failure::Unreadable)?;
                let raw = item.file_name().to_bytes();
                if raw == b"." || raw == b".." {
                    continue;
                }
                if names.len() > self.bound + INVENTORY_EXCLUDED.len() {
                    return Err(Failure::TooLarge);
                }
                names.push(OsStr::from_bytes(raw).to_os_string());
            }
            Ok(names)
        }

        fn children(
            &mut self,
            dir: &OwnedFd,
            depth: usize,
            top: bool,
        ) -> Result<BTreeMap<String, InventoryEntry>, Failure> {
            if depth > MAX_DEPTH {
                return Err(Failure::TooLarge);
            }
            let mut found = BTreeMap::new();
            for raw in self.names(dir)? {
                let name = raw.to_str().ok_or(Failure::Unreadable)?;
                if top && INVENTORY_EXCLUDED.contains(&name) {
                    // Excluded from the inventory, but only as a regular file.
                    if kind(&stat_at(dir, &raw)?) != u32::from(libc::S_IFREG) {
                        return Err(Failure::Special);
                    }
                    continue;
                }
                self.count()?;
                let entry = self.entry(dir, &raw, depth)?;
                found.insert(name.to_string(), entry);
            }
            Ok(found)
        }

        fn entry(
            &mut self,
            dir: &OwnedFd,
            name: &OsStr,
            depth: usize,
        ) -> Result<InventoryEntry, Failure> {
            let before = stat_at(dir, name)?;
            let stat = attributes(&before)?;
            let body = match kind(&before) {
                bits if bits == u32::from(libc::S_IFREG) => EntryBody::File,
                bits if bits == u32::from(libc::S_IFLNK) => {
                    // EINVAL: the link was replaced by something that is not a link.
                    let target = readlinkat(dir, name).map_err(|errno| match errno {
                        Errno::EINVAL => Failure::Changed,
                        other => failure(other),
                    })?;
                    EntryBody::Link(target.to_str().ok_or(Failure::Unreadable)?.to_string())
                }
                bits if bits == u32::from(libc::S_IFDIR) => {
                    EntryBody::Directory(self.directory(dir, name, &stat, depth)?)
                }
                _ => return Err(Failure::Special),
            };
            if attributes(&stat_at(dir, name)?)? != stat {
                return Err(Failure::Changed);
            }
            Ok(InventoryEntry { stat, body })
        }

        fn directory(
            &mut self,
            parent: &OwnedFd,
            name: &OsStr,
            stat: &EntryStat,
            depth: usize,
        ) -> Result<BTreeMap<String, InventoryEntry>, Failure> {
            let fd =
                openat(parent, name, flags(OFlag::O_DIRECTORY), Mode::empty()).map_err(failure)?;
            if attributes(&fstat(&fd).map_err(failure)?)? != *stat {
                return Err(Failure::Changed);
            }
            self.children(&fd, depth + 1, false)
        }
    }
}

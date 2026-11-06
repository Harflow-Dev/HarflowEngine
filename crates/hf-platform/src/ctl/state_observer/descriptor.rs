//! macOS reads relative to a no-follow directory descriptor. The descriptor is checked
//! against the identity of the lstat chain; directories are listed from descriptors
//! opened relative to it, entries are classified with `fstatat(AT_SYMLINK_NOFOLLOW)`
//! (nothing is opened to classify it), and every leaf is opened with
//! `O_NOFOLLOW | O_NONBLOCK` and confirmed regular by `fstat` on the open descriptor.
//! Names stay native bytes until after the last filesystem call that uses them.
use super::{FileId, finish_read};
use hf_domain::ctl::inspect::{EntryKind, FileState, RootEntry};
use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{AtFlags, OFlag, open, openat},
    libc,
    sys::stat::{Mode, fstat, fstatat},
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
    path::Path,
};
pub(super) struct RootHandle {
    fd: OwnedFd,
}
fn flags(extra: OFlag) -> OFlag {
    OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC | extra
}
fn kind_bits(mode: libc::mode_t) -> u32 {
    u32::from(mode) & u32::from(libc::S_IFMT)
}
impl RootHandle {
    pub(super) fn open(path: &Path, id: FileId) -> Option<Self> {
        let fd = open(path, flags(OFlag::O_DIRECTORY), Mode::empty()).ok()?;
        let stat = fstat(&fd).ok()?;
        // The same widening `std::os::unix::fs::MetadataExt::dev` applies.
        let opened = Some((stat.st_dev as u64, stat.st_ino));
        (opened == id).then_some(Self { fd })
    }
    /// Lists `directory` (relative to the root descriptor) from a fresh descriptor, so
    /// the listing is of the pinned directory and not of whatever the path names now.
    /// A name past `limit` ends the listing as truncated.
    fn names(&self, directory: &OsStr, limit: usize) -> Option<(Vec<OsString>, bool)> {
        let fd = openat(
            &self.fd,
            directory,
            flags(OFlag::O_DIRECTORY),
            Mode::empty(),
        )
        .ok()?;
        let mut listing = Dir::from_fd(fd).ok()?;
        let mut names = Vec::new();
        for item in listing.iter() {
            let item = item.ok()?;
            let raw = item.file_name().to_bytes();
            if raw == b"." || raw == b".." {
                continue;
            }
            if names.len() >= limit {
                return Some((names, true));
            }
            names.push(OsStr::from_bytes(raw).to_os_string());
        }
        Some((names, false))
    }
    pub(super) fn entries(&self, limit: usize) -> Option<(Vec<RootEntry>, bool)> {
        let (names, truncated) = self.names(OsStr::new("."), limit)?;
        let mut entries = Vec::new();
        for name in names {
            let Some(kind) = self.kind_of(&name) else {
                continue;
            };
            let empty_directory = kind == EntryKind::Directory && self.is_empty_directory(&name);
            // Only now, after the last filesystem call that needs the exact name.
            entries.push(RootEntry {
                name: name.to_string_lossy().into_owned(),
                kind,
                empty_directory,
            });
        }
        Some((entries, truncated))
    }
    /// Like lstat: a symlink is `Other`. `None` when the entry vanished.
    fn kind_of(&self, name: &OsStr) -> Option<EntryKind> {
        match fstatat(&self.fd, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => Some(match kind_bits(stat.st_mode) {
                bits if bits == u32::from(libc::S_IFREG) => EntryKind::File,
                bits if bits == u32::from(libc::S_IFDIR) => EntryKind::Directory,
                _ => EntryKind::Other,
            }),
            Err(Errno::ENOENT) => None,
            Err(_) => Some(EntryKind::Other),
        }
    }
    fn is_empty_directory(&self, name: &OsStr) -> bool {
        matches!(self.names(name, 0), Some((names, false)) if names.is_empty())
    }
    pub(super) fn read(&self, name: &str, limit: u64, strict: bool) -> FileState {
        let fd = match openat(&self.fd, name, flags(OFlag::empty()), Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::ENOENT) => return FileState::Absent,
            Err(Errno::ELOOP) => return FileState::NotRegular,
            Err(_) => return FileState::Unreadable,
        };
        let Ok(stat) = fstat(&fd) else {
            return FileState::Unreadable;
        };
        if kind_bits(stat.st_mode) != u32::from(libc::S_IFREG) {
            return FileState::NotRegular;
        }
        let size = u64::try_from(stat.st_size).unwrap_or(u64::MAX);
        finish_read(File::from(fd), size, limit, strict)
    }
}

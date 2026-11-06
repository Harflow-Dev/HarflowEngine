//! Path-based reads for targets without the descriptor-relative implementation.
//! Each read lstats, opens, then (on Unix) compares file identity; off Unix the
//! link-aware stat is the only guard. The root and its ancestors are re-resolved by
//! name on every call, so a concurrent replacement can redirect a read. These targets
//! are W02 and WIN02 and are not qualified.
use super::{FileId, file_id, finish_read};
use hf_domain::ctl::inspect::{EntryKind, FileState, RootEntry};
use std::{
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};
pub(super) struct RootHandle(PathBuf);
impl RootHandle {
    pub(super) fn open(path: &Path, id: FileId) -> Option<Self> {
        let current = file_id(&fs::symlink_metadata(path).ok()?);
        (current == id).then(|| Self(path.to_path_buf()))
    }
    pub(super) fn entries(&self, limit: usize) -> Option<(Vec<RootEntry>, bool)> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.0).ok()? {
            if entries.len() >= limit {
                return Some((entries, true));
            }
            let item = item.ok()?;
            let kind = match item.file_type().ok()? {
                kind if kind.is_file() => EntryKind::File,
                kind if kind.is_dir() => EntryKind::Directory,
                _ => EntryKind::Other,
            };
            let empty_directory = kind == EntryKind::Directory
                && fs::read_dir(item.path()).is_ok_and(|mut inner| inner.next().is_none());
            entries.push(RootEntry {
                name: item.file_name().to_string_lossy().into_owned(),
                kind,
                empty_directory,
            });
        }
        Some((entries, false))
    }
    pub(super) fn read(&self, name: &str, limit: u64, strict: bool) -> FileState {
        let path = self.0.join(name);
        let before = match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return FileState::Absent,
            Err(_) => return FileState::Unreadable,
            Ok(metadata) => metadata,
        };
        if !before.file_type().is_file() {
            return FileState::NotRegular;
        }
        let Ok(file) = File::open(&path) else {
            return FileState::Unreadable;
        };
        match file.metadata() {
            Ok(opened) if file_id(&opened) == file_id(&before) => {}
            _ => return FileState::Unreadable,
        }
        finish_read(file, before.len(), limit, strict)
    }
}

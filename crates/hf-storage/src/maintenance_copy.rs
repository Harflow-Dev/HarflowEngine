//! The private copy of the main file that owner evidence is read from when `-wal` is
//! missing. SQLite would create a `-wal` next to the source if it opened it, so the source
//! is never opened; the copy lives in a scratch directory in the root's parent.
use crate::{Error, Result, binding::Binding, connection, small_file};
use hf_application::ctl::state_recover::RecoveryFsPort;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

/// Free space required beyond the size of the main file.
pub(crate) const HEADROOM: u64 = 256 * 1024 * 1024;
/// The files a scratch directory of ours can hold; nothing else is ever removed.
const OURS: [&str; 4] = [
    "store.sqlite3",
    "store.sqlite3-wal",
    "store.sqlite3-shm",
    "store.sqlite3-journal",
];

pub(crate) type Ident = (u64, u64);

/// `.<root-name>.harflow-evidence-<operation>` in the root's parent: outside the root,
/// on its volume, and distinct from the backup path.
///
/// The directory and every file in it are identified when created (or, for the files
/// SQLite adds, right after the connection closed). Removal verifies each recorded
/// identity immediately before the unlink and stops at the first mismatch, deleting
/// nothing further. Clearing a leftover directory of the same name from an earlier
/// attempt follows the same discipline with identities recorded when it is listed, and
/// additionally requires it to be owned by the same user as the store root.
pub(crate) struct Scratch {
    path: PathBuf,
    dir: Ident,
    files: BTreeMap<String, Ident>,
    removed: bool,
}

impl Scratch {
    /// Prepares the scratch directory (mode 0700) for `operation`. A leftover directory
    /// of the same name is ours by name; only the files a copy produces are removed from
    /// it, and anything else makes this fail with nothing deleted.
    pub(crate) fn create(root: &Path, operation: &str) -> Result<Self> {
        Self::create_with(root, operation, &mut |_| {})
    }

    /// `create`, calling `before_unlink(index)` ahead of each leftover deletion's checks.
    pub(crate) fn create_with(
        root: &Path,
        operation: &str,
        before_unlink: &mut dyn FnMut(usize),
    ) -> Result<Self> {
        let parent = root.parent().ok_or(Error::InvalidRoot)?;
        let name = root.file_name().ok_or(Error::InvalidRoot)?;
        Binding::validate_probe_path(parent)?;
        if !same_volume(parent, root)? {
            return Err(Error::InvalidRoot);
        }
        let mut scratch_name = std::ffi::OsString::from(".");
        scratch_name.push(name);
        scratch_name.push(format!(".harflow-evidence-{operation}"));
        let path = parent.join(scratch_name);
        let root_meta = fs::symlink_metadata(root)?;
        if !root_meta.is_dir() {
            return Err(Error::InvalidRoot);
        }
        let owner = owner_of(&root_meta);
        clear_leftover(&path, owner, before_unlink)?;
        make_private_dir(&path)?;
        let dir = ident(&fs::symlink_metadata(&path)?);
        Ok(Self {
            path,
            dir,
            files: BTreeMap::new(),
            removed: false,
        })
    }

    pub(crate) fn database(&self) -> PathBuf {
        self.path.join("store.sqlite3")
    }

    /// Creates the copy target and records its identity.
    pub(crate) fn create_copy(&mut self) -> Result<File> {
        let file = create_private(&self.database())?;
        self.files
            .insert("store.sqlite3".into(), ident(&file.metadata()?));
        Ok(file)
    }

    /// Records the SQLite sidecars next to the copy. Call after the connection closed.
    /// Only after this scratch created the copy itself, only the sidecar names (never the
    /// main file's), and only regular files (`lstat`): a link or anything else stays
    /// unrecorded, so `remove` refuses it.
    pub(crate) fn snapshot(&mut self) -> Result<()> {
        if !self.files.contains_key("store.sqlite3") {
            return Ok(());
        }
        for name in &OURS[1..] {
            if self.files.contains_key(*name) {
                continue;
            }
            match fs::symlink_metadata(self.path.join(name)) {
                Ok(metadata) if metadata.is_file() => {
                    self.files.insert((*name).into(), ident(&metadata));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Removes the copy and the directory. Immediately before every unlink (and the
    /// rmdir) the directory path must still be the directory this scratch created (a
    /// directory, not a link, same device and inode) and the file must still be the one
    /// recorded; a mismatch is `Replaced` and nothing further is deleted.
    ///
    /// Residual (this and `clear_leftover`): the unlink itself is path-based (hf-storage
    /// has no `unlinkat`), so a swap in the instant between the check and the unlink is
    /// not excluded; the scratch directory is private (0700) and created by this process,
    /// and a leftover one must be owned by the store root's owner.
    pub(crate) fn remove(&mut self) -> Result<()> {
        self.remove_with(&mut |_| {})
    }

    /// `remove`, calling `before_unlink(index)` ahead of each deletion's checks.
    pub(crate) fn remove_with(&mut self, before_unlink: &mut dyn FnMut(usize)) -> Result<()> {
        self.removed = true;
        same_dir(&self.path, self.dir)?;
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let want = self.files.get(&name).copied().ok_or(Error::Replaced)?;
            if !OURS.contains(&name.as_str()) {
                return Err(Error::Replaced);
            }
            found.push((entry.path(), want));
        }
        for (index, (path, want)) in found.into_iter().enumerate() {
            before_unlink(index);
            same_dir(&self.path, self.dir)?;
            same_file(&path, want)?;
            fs::remove_file(&path)?;
        }
        same_dir(&self.path, self.dir)?;
        fs::remove_dir(&self.path)?;
        Ok(())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.removed {
            let _ = self.remove();
        }
    }
}

/// `path` must still be the recorded directory: `lstat` says a real directory (not a
/// link) with the recorded device and inode; anything else is `Replaced`.
pub(crate) fn same_dir(path: &Path, want: Ident) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && ident(&metadata) == want {
        Ok(())
    } else {
        Err(Error::Replaced)
    }
}

/// `path` must still be the recorded regular file (`lstat`, same device and inode).
fn same_file(path: &Path, want: Ident) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() && ident(&metadata) == want {
        Ok(())
    } else {
        Err(Error::Replaced)
    }
}

#[cfg(unix)]
fn owner_of(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.uid()
}

#[cfg(not(unix))]
fn owner_of(_: &fs::Metadata) -> u32 {
    0
}

/// A leftover directory is only touched when `owner` (the store root's owner) owns it.
/// `pub(crate)` for tests only.
pub(crate) fn require_owner(metadata: &fs::Metadata, owner: u32) -> Result<()> {
    if owner_of(metadata) == owner {
        Ok(())
    } else {
        Err(Error::InvalidRoot)
    }
}

pub(crate) fn ident(metadata: &fs::Metadata) -> Ident {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (0, 0)
    }
}

/// Copies the root's `store.sqlite3` into `output`, bounded by `deadline`. The source is
/// the regular file `lstat` lists (a swapped file or link is refused); free space of
/// `parent` must cover its size and `HEADROOM`.
pub(crate) fn copy_main(
    root: &Path,
    mut output: File,
    parent: &Path,
    fs_port: &dyn RecoveryFsPort,
    deadline: Instant,
) -> Result<()> {
    connection::check_deadline(deadline)?;
    let (mut input, size) =
        small_file::open_listed(root, "store.sqlite3", false)?.ok_or(Error::Incompatible)?;
    if fs_port.free_space(parent)? < size.saturating_add(HEADROOM) {
        return Err(Error::LimitExceeded);
    }
    let mut buffer = vec![0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        connection::check_deadline(deadline)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > size {
            return Err(Error::Replaced);
        }
        output.write_all(&buffer[..count])?;
    }
    if copied != size {
        return Err(Error::Replaced);
    }
    Ok(())
}

#[cfg(unix)]
fn create_private(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

#[cfg(unix)]
fn make_private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new().create(path)?;
    Ok(())
}

#[cfg(unix)]
fn same_volume(parent: &Path, root: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::symlink_metadata(parent)?.dev() == fs::symlink_metadata(root)?.dev())
}

#[cfg(not(unix))]
fn same_volume(_: &Path, _: &Path) -> Result<bool> {
    Ok(false)
}

/// Removes `path` when it is a plain directory owned by `owner` holding only files a copy
/// produces; a missing path is fine; a link, a file or anything else inside, or a foreign
/// owner, makes it fail untouched (`InvalidRoot`). The directory's and each file's
/// identity is recorded when listed and verified immediately before every unlink and the
/// rmdir; a mismatch is `Replaced` and nothing further is deleted. `before_unlink(index)`
/// runs ahead of each deletion's checks.
fn clear_leftover(path: &Path, owner: u32, before_unlink: &mut dyn FnMut(usize)) -> Result<()> {
    let dir = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() => return Err(Error::InvalidRoot),
        Ok(metadata) => {
            require_owner(&metadata, owner)?;
            ident(&metadata)
        }
    };
    let mut found = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        if !OURS.iter().any(|ours| name == *ours) {
            return Err(Error::InvalidRoot);
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() {
            return Err(Error::InvalidRoot);
        }
        found.push((entry.path(), ident(&metadata)));
    }
    for (index, (file, want)) in found.into_iter().enumerate() {
        before_unlink(index);
        same_dir(path, dir)?;
        same_file(&file, want)?;
        fs::remove_file(file)?;
    }
    same_dir(path, dir)?;
    fs::remove_dir(path)?;
    Ok(())
}

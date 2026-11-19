use super::{refused, timely};
use hf_application::{durability::FlushPort, lease::LeaseMode};
use hf_domain::{capture::FileIdentity, task_lifecycle::LifecycleBinding};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};
pub(super) struct RootPath {
    entries: Vec<(PathBuf, File, FileIdentity)>,
}
fn identity(m: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: m.dev(),
        inode: m.ino(),
    }
}
/// An opened directory that passed the private predicate: owned by the current user with
/// no group or other permission bits. The descriptor pins the identity it was checked on.
pub struct PrivateRoot {
    file: File,
    identity: FileIdentity,
}
impl PrivateRoot {
    pub fn identity(&self) -> &FileIdentity {
        &self.identity
    }
    pub fn file(&self) -> &File {
        &self.file
    }
}
/// Why a directory failed the private predicate, observed at the refusing step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateRootError {
    /// A directory of the current user with group or other permission bits.
    LooseMode,
    /// Not absolute, not at its canonical path, or a symlink.
    NotCanonical,
    NotDirectory,
    ForeignOwner,
    /// Anything else: a missing path, an access error.
    Io,
}
impl std::fmt::Display for PrivateRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "private root refused: {self:?}")
    }
}
impl std::error::Error for PrivateRootError {}
fn typed(kind: PrivateRootError) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, kind)
}
/// The classification carried by an error of `open_private_root`.
pub fn private_root_error(error: &io::Error) -> PrivateRootError {
    if let Some(kind) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<PrivateRootError>())
    {
        return *kind;
    }
    match error.raw_os_error() {
        Some(nix::libc::ELOOP) => PrivateRootError::NotCanonical,
        Some(nix::libc::ENOTDIR) => PrivateRootError::NotDirectory,
        _ => PrivateRootError::Io,
    }
}
/// The single root predicate shared by dispatch admission, the writer and the retained
/// cause record: an absolute, canonical path opened without following a final symlink,
/// whose descriptor is a directory owned by the current user with `mode & 0o077 == 0`.
/// A refusal carries its `PrivateRootError`.
pub fn open_private_root(path: &Path) -> io::Result<PrivateRoot> {
    if !path.is_absolute() || fs::canonicalize(path)? != path {
        return Err(typed(PrivateRootError::NotCanonical));
    }
    open_private_dir(path)
}
/// A path component the raw payload tree may contain.
pub(super) fn valid_name(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 255
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub(super) fn open_private_dir(path: &Path) -> io::Result<PrivateRoot> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_DIRECTORY)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_dir() {
        return Err(typed(PrivateRootError::NotDirectory));
    }
    if m.uid() != nix::unistd::getuid().as_raw() {
        return Err(typed(PrivateRootError::ForeignOwner));
    }
    if m.mode() & 0o077 != 0 {
        return Err(typed(PrivateRootError::LooseMode));
    }
    Ok(PrivateRoot {
        identity: identity(&m),
        file,
    })
}
impl RootPath {
    pub fn create(root: &Path, binding: &LifecycleBinding, deadline: Instant) -> io::Result<Self> {
        if !root.is_absolute() || fs::canonicalize(root)? != root {
            return Err(refused());
        }
        let mut value = Self {
            entries: Vec::new(),
        };
        value.retain(root.to_path_buf())?;
        let task = binding.task.as_str();
        let parts = [
            "private-payload",
            binding.generation.as_str(),
            task,
            binding.attempt.as_str(),
        ];
        let mut path = root.to_path_buf();
        for part in parts {
            timely(deadline)?;
            if !valid_name(part) {
                return Err(refused());
            }
            path.push(part);
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    let parent = path.parent().ok_or_else(refused)?;
                    crate::durability::FullFlush.flush(&File::open(parent)?)?;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
            value.retain(path.clone())?;
        }
        value.validate()?;
        timely(deadline)?;
        Ok(value)
    }
    fn retain(&mut self, path: PathBuf) -> io::Result<()> {
        let PrivateRoot { file, identity } = open_private_dir(&path)?;
        self.entries.push((path, file, identity));
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.entries[self.entries.len() - 1].0
    }
    pub fn validate(&self) -> io::Result<()> {
        for (path, file, expected) in &self.entries {
            for m in [file.metadata()?, fs::symlink_metadata(path)?] {
                if !m.is_dir()
                    || m.uid() != nix::unistd::getuid().as_raw()
                    || m.mode() & 0o077 != 0
                    || identity(&m) != *expected
                {
                    return Err(refused());
                }
            }
        }
        Ok(())
    }
    pub fn sync(&self) -> io::Result<()> {
        for (_, file, _) in &self.entries {
            crate::durability::FullFlush.flush(file)?;
        }
        Ok(())
    }
}
pub(crate) struct RawFile {
    _lease: Box<dyn hf_application::lease::LeaseGuard>,
    file: File,
    path: PathBuf,
    identity: FileIdentity,
    cap: u64,
    strict: bool,
}
impl RawFile {
    pub fn create(path: PathBuf, deadline: Instant) -> io::Result<Self> {
        let sentinel = path.with_extension("lock");
        Self::create_qualified(path, sentinel, super::RAW_CAP, true, deadline)
    }
    pub fn create_qualified(
        path: PathBuf,
        sentinel: PathBuf,
        cap: u64,
        strict: bool,
        deadline: Instant,
    ) -> io::Result<Self> {
        let lease = crate::lease::shared()
            .acquire(&sentinel, LeaseMode::Exclusive, true, deadline)
            .map_err(|_| refused())?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(if strict { 0o600 } else { 0o666 })
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)?;
        let identity = identity(&file.metadata()?);
        let value = Self {
            _lease: lease,
            file,
            path,
            identity,
            cap,
            strict,
        };
        value.validate(0)?;
        if strict {
            crate::durability::FullFlush.flush(&value.file)?;
        }
        timely(deadline)?;
        Ok(value)
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity.clone()
    }
    pub fn validate(&self, length: u64) -> io::Result<()> {
        for m in [self.file.metadata()?, fs::symlink_metadata(&self.path)?] {
            if !m.is_file()
                || m.nlink() != 1
                || m.ino() == 0
                || m.len() != length
                || length > self.cap
                || (self.strict
                    && (m.uid() != nix::unistd::getuid().as_raw() || m.mode() & 0o077 != 0))
                || identity(&m) != self.identity
            {
                return Err(refused());
            }
        }
        Ok(())
    }
    /// Reads the first `length` bytes through the retained descriptor.
    pub fn read_prefix(&self, length: usize) -> io::Result<Vec<u8>> {
        let mut bytes = vec![0; length];
        std::os::unix::fs::FileExt::read_exact_at(&self.file, &mut bytes, 0)?;
        Ok(bytes)
    }
    pub fn write_unflushed(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }
    pub fn sync(&self) -> io::Result<()> {
        crate::durability::FullFlush.flush(&self.file)
    }
    pub fn append(&mut self, length: u64, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        timely(deadline)?;
        self.validate(length)?;
        self.file.write_all(bytes)?;
        crate::durability::FullFlush.flush(&self.file)?;
        self.validate(length + bytes.len() as u64)?;
        timely(deadline)
    }
}

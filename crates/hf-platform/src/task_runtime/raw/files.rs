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
            if part.is_empty()
                || part.len() > 255
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
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
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_DIRECTORY)
            .open(&path)?;
        let m = file.metadata()?;
        if !m.is_dir() || m.uid() != nix::unistd::getuid().as_raw() || m.mode() & 0o077 != 0 {
            return Err(refused());
        }
        self.entries.push((path, file, identity(&m)));
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

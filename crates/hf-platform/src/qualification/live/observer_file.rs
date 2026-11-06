//! Retained, bounded private diagnostic metadata; never CLI capture input.
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub struct ObserverFile {
    file: File,
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl ObserverFile {
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(path)?;
        let stamp = file.metadata()?;
        let value = Self {
            file,
            path: path.to_owned(),
            device: stamp.dev(),
            inode: stamp.ino(),
        };
        value.validate(0)?;
        Ok(value)
    }
    pub fn device(&self) -> u64 {
        self.device
    }
    pub fn inode(&self) -> u64 {
        self.inode
    }
    fn valid(&self, stamp: &fs::Metadata, maximum: u64) -> bool {
        stamp.is_file()
            && !stamp.file_type().is_symlink()
            && stamp.nlink() == 1
            && stamp.dev() == self.device
            && stamp.ino() == self.inode
            && stamp.len() <= maximum
            && stamp.mode() & 0o777 == 0o600
    }
    fn validate(&self, maximum: u64) -> io::Result<()> {
        if self.valid(&self.file.metadata()?, maximum)
            && self.valid(&fs::symlink_metadata(&self.path)?, maximum)
        {
            Ok(())
        } else {
            Err(io::Error::other("observer file identity or bound refused"))
        }
    }
    pub fn read_bounded(&self) -> io::Result<Vec<u8>> {
        self.validate(512)?;
        let before = self.file.metadata()?;
        let mut bytes = [0; 513];
        let mut length = 0;
        while length < bytes.len() {
            let count = self.file.read_at(&mut bytes[length..], length as u64)?;
            if count == 0 {
                break;
            }
            length += count;
        }
        self.validate(512)?;
        let after = self.file.metadata()?;
        if length > 512
            || length as u64 != after.len()
            || before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(io::Error::other("observer file unstable or over bound"));
        }
        Ok(bytes[..length].to_vec())
    }
}

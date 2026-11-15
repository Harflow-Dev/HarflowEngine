//! Filesystem effects of a state recovery that need descriptors or the volume.
use hf_application::ctl::state_recover::RecoveryFsPort;
use nix::{
    errno::Errno,
    fcntl::{OFlag, open},
    sys::{stat::Mode, statvfs::statvfs},
};
use std::{io, path::Path};

/// The macOS adapter of `RecoveryFsPort`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MacRecoveryFs;

impl RecoveryFsPort for MacRecoveryFs {
    fn create_lock(&self, root: &Path, name: &str) -> io::Result<bool> {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let flags = OFlag::O_RDWR
            | OFlag::O_CREAT
            | OFlag::O_EXCL
            | OFlag::O_NOFOLLOW
            | OFlag::O_NONBLOCK
            | OFlag::O_CLOEXEC;
        match open(&root.join(name), flags, Mode::from_bits_truncate(0o600)) {
            Ok(descriptor) => {
                drop(descriptor);
                Ok(true)
            }
            Err(Errno::EEXIST) => Ok(false),
            Err(error) => Err(io::Error::from(error)),
        }
    }

    fn free_space(&self, path: &Path) -> io::Result<u64> {
        let stat = statvfs(path).map_err(io::Error::from)?;
        Ok(u64::from(stat.blocks_available()).saturating_mul(stat.fragment_size()))
    }
}

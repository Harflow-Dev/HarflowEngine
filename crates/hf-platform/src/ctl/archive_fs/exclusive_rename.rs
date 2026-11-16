//! The approved safe Darwin atomic no-overwrite rename seam.
use super::ExclusiveRenamePort;
use std::{io, os::fd::OwnedFd};

#[derive(Clone, Copy, Debug, Default)]
pub struct MacExclusiveRename;

impl ExclusiveRenamePort for MacExclusiveRename {
    fn rename_exclusive(
        &self,
        source: &OwnedFd,
        name: &str,
        destination: &OwnedFd,
        target: &str,
    ) -> io::Result<()> {
        if !basename(name) || !basename(target) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        rustix::fs::renameat_with(
            source,
            name,
            destination,
            target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from)
    }
}

fn basename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

use super::{text_path, timely};
use hf_application::prepared_launch::{StreamDigestPort, StreamingDigestFactory};
use hf_domain::{
    capture::FileIdentity,
    prepared_launch::{BinaryMetadata, BinaryPin},
    task_launch::LaunchRefusal,
};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Instant,
};

// Bounds observation work; this is not an execution lifetime or capture budget.
const BINARY_LIMIT: u64 = 512 * 1024 * 1024;
pub(super) fn observe(
    physical: &Path,
    factory: &impl StreamingDigestFactory,
    deadline: Instant,
) -> Result<BinaryPin, LaunchRefusal> {
    timely(deadline)?;
    if fs::canonicalize(physical).map_err(|_| LaunchRefusal::Unavailable)? != physical {
        return Err(LaunchRefusal::IdentityChanged);
    }
    let before = stamp(&fs::symlink_metadata(physical).map_err(|_| LaunchRefusal::Unavailable)?)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(physical)
        .map_err(|_| LaunchRefusal::Unavailable)?;
    if stamp(&file.metadata().map_err(|_| LaunchRefusal::Unavailable)?)? != before {
        return Err(LaunchRefusal::IdentityChanged);
    }
    let mut hash = factory
        .start(BINARY_LIMIT as usize)
        .map_err(|_| LaunchRefusal::Unavailable)?;
    let mut bytes = [0_u8; 16384];
    let mut count = 0_u64;
    loop {
        timely(deadline)?;
        let n = file
            .read(&mut bytes)
            .map_err(|_| LaunchRefusal::Unavailable)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > before.length {
            return Err(LaunchRefusal::IdentityChanged);
        }
        hash.update(&bytes[..n])
            .map_err(|_| LaunchRefusal::Unavailable)?;
    }
    timely(deadline)?;
    if count != before.length
        || stamp(&file.metadata().map_err(|_| LaunchRefusal::Unavailable)?)? != before
        || stamp(&fs::symlink_metadata(physical).map_err(|_| LaunchRefusal::Unavailable)?)?
            != before
    {
        return Err(LaunchRefusal::IdentityChanged);
    }
    let path = text_path(physical)?;
    Ok(BinaryPin {
        logical_path: path.clone(),
        physical_path: path,
        metadata: before,
        digest: hash.finish().map_err(|_| LaunchRefusal::Unavailable)?,
    })
}
fn stamp(value: &Metadata) -> Result<BinaryMetadata, LaunchRefusal> {
    if !value.is_file()
        || value.nlink() != 1
        || value.ino() == 0
        || value.len() == 0
        || value.len() > BINARY_LIMIT
        || value.mode() & 0o111 == 0
    {
        return Err(LaunchRefusal::InvalidDiscovery);
    }
    Ok(BinaryMetadata {
        identity: FileIdentity {
            device: value.dev(),
            inode: value.ino(),
        },
        length: value.len(),
        modified_seconds: value.mtime(),
        modified_nanoseconds: value.mtime_nsec(),
        changed_seconds: value.ctime(),
        changed_nanoseconds: value.ctime_nsec(),
    })
}

//! Bounded on-disk observations; neither atomic fd-exec nor mapped-image proof.
use hf_application::{
    prepared_launch::{StreamDigestPort, StreamingDigestFactory},
    recovery::RecoveryError,
};
use hf_domain::{
    capture::FileIdentity,
    prepared_launch::{BinaryMetadata, BinaryPin},
};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Instant,
};

pub fn observe_binary(
    path: &Path,
    factory: &impl StreamingDigestFactory,
    deadline: Instant,
) -> Result<BinaryPin, RecoveryError> {
    timely(deadline)?;
    let physical = fs::canonicalize(path).map_err(unavailable)?;
    let before = fs::symlink_metadata(path).map_err(unavailable)?;
    let stamp = metadata(&before)?;
    if stamp.length == 0 || stamp.length > 128 * 1024 * 1024 {
        return Err(RecoveryError::Refused);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(unavailable)?;
    if metadata(&file.metadata().map_err(unavailable)?)? != stamp {
        return Err(RecoveryError::Stale);
    }
    let mut hash = factory.start(128 * 1024 * 1024)?;
    let mut bytes = [0_u8; 16384];
    let mut count = 0_u64;
    loop {
        timely(deadline)?;
        let n = file.read(&mut bytes).map_err(unavailable)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > stamp.length {
            return Err(RecoveryError::Stale);
        }
        StreamDigestPort::update(&mut hash, &bytes[..n])?;
    }
    timely(deadline)?;
    if count != stamp.length
        || metadata(&file.metadata().map_err(unavailable)?)? != stamp
        || metadata(&fs::symlink_metadata(path).map_err(unavailable)?)? != stamp
        || fs::canonicalize(path).map_err(unavailable)? != physical
    {
        return Err(RecoveryError::Stale);
    }
    Ok(BinaryPin {
        logical_path: path.to_str().ok_or(RecoveryError::Invalid)?.into(),
        physical_path: physical.to_str().ok_or(RecoveryError::Invalid)?.into(),
        metadata: stamp,
        digest: StreamDigestPort::finish(hash)?,
    })
}
pub(crate) fn metadata(value: &Metadata) -> Result<BinaryMetadata, RecoveryError> {
    if !value.is_file() || value.nlink() != 1 || value.ino() == 0 {
        return Err(RecoveryError::Refused);
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
pub(crate) fn timely(deadline: Instant) -> Result<(), RecoveryError> {
    if Instant::now() >= deadline {
        Err(RecoveryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
pub(crate) fn unavailable(_: std::io::Error) -> RecoveryError {
    RecoveryError::Unavailable
}

pub struct ExactBinaryObserver<H> {
    hash: H,
}
impl<H> ExactBinaryObserver<H> {
    pub fn new(hash: H) -> Self {
        Self { hash }
    }
}
impl<H: StreamingDigestFactory> hf_application::prepared_launch::ExactBinaryPort
    for ExactBinaryObserver<H>
{
    fn observe(&self, logical_path: &str, deadline: Instant) -> Result<BinaryPin, RecoveryError> {
        observe_binary(Path::new(logical_path), &self.hash, deadline)
    }
}

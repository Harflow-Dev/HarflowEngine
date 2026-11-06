//! Exact finite raw extents. No retry, tailing, creation, or repair.
use crate::prepared_launch::{metadata, timely, unavailable};
use hf_application::{prepared_launch::ExactRawPort, recovery::RecoveryError};
use hf_domain::capture::{CaptureBinding, CaptureProgress, FileIdentity};
use hf_domain::prepared_launch::RawBytes;
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::Instant,
};
pub struct ExactRawCaptureReader {
    root: PathBuf,
}
impl ExactRawCaptureReader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}
impl ExactRawPort for ExactRawCaptureReader {
    fn read_exact(
        &self,
        binding: &CaptureBinding,
        progress: &CaptureProgress,
        deadline: Instant,
    ) -> Result<RawBytes, RecoveryError> {
        let parent = self.root.parent().ok_or(RecoveryError::Invalid)?;
        let base = format!("r03-b2c.{}", binding.fence.operation.as_str());
        let stdout = read(
            &parent.join(format!("{base}.stdout")),
            &binding.stdout,
            progress.stdout,
            deadline,
        )?;
        let stderr = read(
            &parent.join(format!("{base}.stderr")),
            &binding.stderr,
            progress.stderr,
            deadline,
        )?;
        Ok(RawBytes { stdout, stderr })
    }
}
fn read(
    path: &Path,
    identity: &FileIdentity,
    extent: u64,
    deadline: Instant,
) -> Result<Vec<u8>, RecoveryError> {
    timely(deadline)?;
    if extent > 65536 {
        return Err(RecoveryError::Refused);
    }
    let before = metadata(&fs::symlink_metadata(path).map_err(unavailable)?)?;
    if &before.identity != identity || before.length != extent {
        return Err(RecoveryError::Stale);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(unavailable)?;
    if metadata(&file.metadata().map_err(unavailable)?)? != before {
        return Err(RecoveryError::Stale);
    }
    let mut bytes = Vec::with_capacity(extent as usize);
    let mut chunk = [0_u8; 4096];
    while bytes.len() < extent as usize {
        timely(deadline)?;
        let size = chunk.len().min(extent as usize - bytes.len());
        let n = file.read(&mut chunk[..size]).map_err(unavailable)?;
        if n == 0 {
            return Err(RecoveryError::Stale);
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    timely(deadline)?;
    if metadata(&file.metadata().map_err(unavailable)?)? != before
        || metadata(&fs::symlink_metadata(path).map_err(unavailable)?)? != before
    {
        return Err(RecoveryError::Stale);
    }
    Ok(bytes)
}

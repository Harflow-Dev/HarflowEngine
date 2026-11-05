use super::timely;
use hf_application::recovery::{RecoveryError, RecoveryRawPort};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, FileIdentity},
    recovery::{RawEvidence, StreamObservation},
};
use std::{
    fs::{self, Metadata, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

pub struct RawCaptureObserver {
    root: PathBuf,
}
impl RawCaptureObserver {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}
impl RecoveryRawPort for RawCaptureObserver {
    fn observe(
        &self,
        binding: &CaptureBinding,
        progress: &CaptureProgress,
        deadline: Instant,
    ) -> Result<RawEvidence, RecoveryError> {
        timely(deadline)?;
        let parent = self.root.parent().ok_or(RecoveryError::Invalid)?;
        let name = format!("r03-b2c.{}", binding.fence.operation.as_str());
        let stdout = observe_file(
            &parent.join(format!("{name}.stdout")),
            &binding.stdout,
            progress.stdout,
        );
        timely(deadline)?;
        let stderr = observe_file(
            &parent.join(format!("{name}.stderr")),
            &binding.stderr,
            progress.stderr,
        );
        timely(deadline)?;
        Ok(RawEvidence { stdout, stderr })
    }
}

fn observe_file(path: &Path, expected: &FileIdentity, offset: u64) -> StreamObservation {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return classify_error(error),
    };
    let result = (|| {
        if !valid(&before, expected, offset) {
            return Ok(StreamObservation::Changed);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)?;
        let opened = file.metadata()?;
        let after = fs::symlink_metadata(path)?;
        let final_open = file.metadata()?;
        if [&opened, &after, &final_open]
            .iter()
            .any(|m| !valid(m, expected, offset) || m.len() != before.len())
        {
            return Ok(StreamObservation::Changed);
        }
        Ok(StreamObservation::Present {
            identity: expected.clone(),
            length: before.len(),
        })
    })();
    result.unwrap_or_else(|error: io::Error| {
        if error.kind() == io::ErrorKind::NotFound {
            StreamObservation::Changed
        } else {
            classify_error(error)
        }
    })
}
fn valid(metadata: &Metadata, expected: &FileIdentity, offset: u64) -> bool {
    metadata.is_file()
        && metadata.nlink() == 1
        && metadata.ino() != 0
        && metadata.dev() == expected.device
        && metadata.ino() == expected.inode
        && metadata.len() <= 65536
        && metadata.len() >= offset
}
fn classify_error(error: io::Error) -> StreamObservation {
    if error.kind() == io::ErrorKind::NotFound {
        StreamObservation::Missing
    } else if error.raw_os_error() == Some(nix::libc::ELOOP) {
        StreamObservation::Changed
    } else {
        StreamObservation::Unavailable
    }
}

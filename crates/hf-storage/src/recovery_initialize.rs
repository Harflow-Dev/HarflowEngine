//! The files-only initializer bound to the original maintenance exclusion.
use crate::{Error, MaintenanceSession, Result, initialize_files};
use hf_application::{
    ctl::state_recover::{flow::RecoveryError, initialization::FreshStorePort},
    durability::FlushPort,
};
use hf_domain::ctl::{recovery_proposal::FileIdentity, state_recovery::is_operation_id};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub struct RecoveryInitializer<'a> {
    session: &'a MaintenanceSession,
    flush: &'a dyn FlushPort,
}
impl<'a> RecoveryInitializer<'a> {
    pub fn new(session: &'a MaintenanceSession, flush: &'a dyn FlushPort) -> Self {
        Self { session, flush }
    }
}
impl FreshStorePort for RecoveryInitializer<'_> {
    fn initialize(
        &self,
        existing_dir: &Path,
        expected: FileIdentity,
        deadline: Instant,
    ) -> std::result::Result<(), RecoveryError> {
        let verify = || {
            self.session.verify()?;
            let operation = existing_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(".harflow-recovery-init-"))
                .ok_or(Error::InvalidRoot)?;
            if existing_dir.parent() != Some(self.session.root()) || !is_operation_id(operation) {
                return Err(Error::InvalidRoot);
            }
            directory_identity(existing_dir, expected)
        };
        verify().map_err(|_| RecoveryError::BackupConflict)?;
        if std::fs::read_dir(existing_dir)
            .map_err(|_| RecoveryError::RecoveryInterrupted)?
            .next()
            .is_some()
        {
            return Err(RecoveryError::BackupConflict);
        }
        initialize_files(
            existing_dir,
            self.flush,
            deadline.min(Instant::now() + Duration::from_secs(10)),
            &verify,
        )
        .map(|_| ())
        .map_err(|error| match error {
            Error::Replaced | Error::InvalidRoot => RecoveryError::BackupConflict,
            _ => RecoveryError::RecoveryInterrupted,
        })
    }
}

#[cfg(unix)]
fn directory_identity(path: &Path, expected: FileIdentity) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.dev() as i64 != expected.dev || metadata.ino() != expected.ino
    {
        return Err(Error::Replaced);
    }
    Ok(())
}
#[cfg(not(unix))]
fn directory_identity(_: &Path, _: FileIdentity) -> Result<()> {
    Err(Error::InvalidRoot)
}

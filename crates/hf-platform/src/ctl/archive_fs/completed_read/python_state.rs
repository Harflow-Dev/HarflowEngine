//! Bounded positive Python-state checks before creating maintenance infrastructure.
use super::*;
use hf_domain::ctl::{
    inspect::JournalDecode,
    recovery_proposal::{REFERENCE_STATE_DIRECTORIES, RecoveryProposal},
};
use nix::{dir::Dir, errno::Errno, fcntl::openat, libc, sys::stat::Mode};
impl FsRecoveryRead<'_> {
    /// Marker and known legacy directory observations only; never reads a journal/store.
    pub fn refuse_python_state(&self, proposal: &RecoveryProposal) -> Result<(), RecoveryError> {
        let root = self.proposal_root(proposal)?;
        if read(&root, ".harflow-store.json", 16_384)?.is_some_and(|bytes| {
            matches!(self.codec.decode_marker(&bytes), MarkerDecode::Store { .. })
        }) {
            return Err(RecoveryError::PythonState);
        }
        for name in REFERENCE_STATE_DIRECTORIES {
            if let Some(before) = stat(&root, name)? {
                if u32::from(before.st_mode) & u32::from(libc::S_IFMT) != u32::from(libc::S_IFDIR) {
                    continue;
                }
                let dir = admission_directory(&root, name)?;
                if identity(&dir)? != id(&before) {
                    return Err(RecoveryError::StaleConfirmation);
                }
                let mut listing = Dir::from_fd(dir).map_err(failure)?;
                for item in listing.iter().take(3) {
                    let item = item.map_err(failure)?;
                    if ![b".".as_slice(), b"..".as_slice()].contains(&item.file_name().to_bytes()) {
                        return Err(RecoveryError::PythonState);
                    }
                }
                if stat(&root, name)?.as_ref().map(id) != Some(id(&before)) {
                    return Err(RecoveryError::StaleConfirmation);
                }
            }
        }
        self.proposal_root(proposal).map(|_| ())
    }
    /// Call only after historical receipt lookup returned none. No inventory or SQLite.
    pub fn refuse_python_journal(&self, proposal: &RecoveryProposal) -> Result<(), RecoveryError> {
        let root = self.proposal_root(proposal)?;
        let bytes = match read(&root, ".harflow-recovery.json.pending", JOURNAL_BOUND)? {
            Some(bytes) => Some(bytes),
            None => read(&root, ".harflow-recovery.json", JOURNAL_BOUND)?,
        };
        if let Some(bytes) = bytes
            && let JournalDecode::Qualified {
                proposal: Some(journal_proposal),
                ..
            } = self.codec.decode_journal(&bytes, self.root)
            && self.codec.journal_engine_version(&bytes).as_deref()
                == Some(journal_proposal.engine_version.as_str())
            && python_release(&journal_proposal.engine_version)
        {
            return Err(RecoveryError::PythonState);
        }
        self.proposal_root(proposal).map(|_| ())
    }
    fn proposal_root(&self, proposal: &RecoveryProposal) -> Result<OwnedFd, RecoveryError> {
        let root = self.root()?;
        if proposal.state_root != self.root.as_str() || identity(&root)? != proposal.root_identity {
            return Err(RecoveryError::StaleConfirmation);
        }
        Ok(root)
    }
}
/// Python publishes unadorned numeric releases. Rust versions retain their +rust
/// producer suffix; an unknown/non-release string proves neither producer.
fn python_release(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 10 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// This is an observation, not a backup opener: inaccessible contents remain unknown.
fn admission_directory(root: &OwnedFd, name: &str) -> Result<OwnedFd, RecoveryError> {
    openat(root, name, flags(), Mode::empty()).map_err(|error| match error {
        Errno::ENOENT => RecoveryError::StaleConfirmation,
        Errno::ELOOP | Errno::ENOTDIR => RecoveryError::UnsafeRoot,
        _ => RecoveryError::InfrastructureUnavailable,
    })
}

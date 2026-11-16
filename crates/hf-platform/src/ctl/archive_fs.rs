//! Descriptor-relative internal archive effects. No public ctl caller is composed.
//! This mutation port requires prior `flow::recover` admission through the production
//! state observer and original held session. It is not general recovery authority;
//! dedicated-root policy (including filesystem root and account home) is owned by
//! that observer and is checked by the flow before pending publication.
mod descriptor;
mod exclusive_rename;
pub use exclusive_rename::MacExclusiveRename;
mod locations;
mod publication;
mod skeleton;
use hf_application::ctl::state_recover::{
    binding::HeldSessionPort,
    flow::{ArchiveMutationPort, JournalCodecPort, RecoveryError},
    journal::RecoveryJournal,
};
use hf_domain::ctl::recovery_proposal::{FileIdentity, InventoryEntry};
use std::{io, os::fd::OwnedFd};

/// Must atomically refuse an occupied destination; check-then-rename is forbidden.
/// The implementation must use Darwin RENAME_EXCL and never follow either name.
pub trait ExclusiveRenamePort {
    fn rename_exclusive(
        &self,
        source: &OwnedFd,
        name: &str,
        destination: &OwnedFd,
        target: &str,
    ) -> io::Result<()>;
}
/// Called separately immediately before each effect, then bindings are rechecked.
pub trait ArchiveFaultPort {
    fn before(&self, boundary: &'static str) -> io::Result<()>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoArchiveFault;
impl ArchiveFaultPort for NoArchiveFault {
    fn before(&self, _: &'static str) -> io::Result<()> {
        Ok(())
    }
}

pub struct FsArchive<'a> {
    session: &'a dyn HeldSessionPort,
    rename: &'a dyn ExclusiveRenamePort,
    faults: &'a dyn ArchiveFaultPort,
    codec: &'a dyn JournalCodecPort,
}
impl<'a> FsArchive<'a> {
    pub fn new(
        session: &'a dyn HeldSessionPort,
        rename: &'a dyn ExclusiveRenamePort,
        faults: &'a dyn ArchiveFaultPort,
        codec: &'a dyn JournalCodecPort,
    ) -> Self {
        Self {
            session,
            rename,
            faults,
            codec,
        }
    }
}
impl ArchiveMutationPort for FsArchive<'_> {
    fn staging_absent(&self, operation_id: &str) -> Result<(), RecoveryError> {
        let root = self.root()?;
        descriptor::absent(&root, &format!(".harflow-recovery-init-{operation_id}"))
    }
    fn backup_absent(&self, path: &str) -> Result<(), RecoveryError> {
        let (parent, name) = descriptor::parent(path)?;
        descriptor::absent(&parent, &name)
    }
    fn write_pending(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.pending(j)
    }
    fn write_journal(&self, j: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError> {
        let root = self.root()?;
        self.publish(
            j,
            &root,
            ".harflow-recovery.json",
            ".harflow-recovery.json.pending",
            bytes,
            false,
        )
    }
    fn prepare_backup(
        &self,
        j: &RecoveryJournal,
        codec: &dyn JournalCodecPort,
    ) -> Result<(FileIdentity, FileIdentity), RecoveryError> {
        self.prepare(j, codec)
    }
    fn validate_locations(
        &self,
        j: &RecoveryJournal,
        final_only: bool,
    ) -> Result<(), RecoveryError> {
        self.locations(j, final_only)
    }
    fn move_entry(
        &self,
        j: &RecoveryJournal,
        name: &str,
        expected: &InventoryEntry,
    ) -> Result<(), RecoveryError> {
        self.move_one(j, name, expected)
    }
    fn move_shm(&self, j: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.shm(j)
    }
    fn write_manifest(&self, j: &RecoveryJournal, bytes: &[u8]) -> Result<(), RecoveryError> {
        let dirs = self.dirs(j)?;
        self.publish(
            j,
            &dirs.backup,
            "manifest.json",
            "manifest.json.pending",
            bytes,
            false,
        )
    }
}

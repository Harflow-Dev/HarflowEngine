//! Internal fresh-file initialization and metadata-only staged publication ports.
use super::{
    flow::RecoveryError,
    journal::{RecoveryJournal, StagedFile},
};
use hf_domain::ctl::recovery_proposal::FileIdentity;
use std::{collections::BTreeMap, path::Path, time::Instant};

/// Storage owns SQLite, under the original maintenance session's guards.
pub trait FreshStorePort {
    fn initialize(
        &self,
        existing_dir: &Path,
        expected: FileIdentity,
        deadline: Instant,
    ) -> Result<(), RecoveryError>;
}

pub trait StagingMutationPort {
    /// Rechecks original archive identities and root entries; never opens SQLite.
    fn validate_archive(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    /// Cleans an interrupted initializer only after proving directory ownership and
    /// the complete fixed regular-file set. Unknown entries cause no deletion.
    fn cleanup_staging(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn prepare_staging(&self, journal: &RecoveryJournal) -> Result<FileIdentity, RecoveryError>;
    /// The sole SQLite call, followed by close, file flushes and a directory barrier.
    fn initialize_staging(
        &self,
        journal: &RecoveryJournal,
    ) -> Result<BTreeMap<String, StagedFile>, RecoveryError>;
    /// Metadata/hash-only replay, with identity publication last.
    fn publish_staged(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
    fn validate_initialized(&self, journal: &RecoveryJournal) -> Result<(), RecoveryError>;
}

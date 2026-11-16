use super::{descriptor::*, *};
use hf_application::ctl::state_recover::journal::{JOURNAL_BOUND, JournalPhase};
use nix::{
    errno::Errno,
    sys::stat::{Mode, mkdirat},
};

impl FsArchive<'_> {
    pub(super) fn prepare(
        &self,
        j: &RecoveryJournal,
        codec: &dyn JournalCodecPort,
    ) -> Result<(FileIdentity, FileIdentity), RecoveryError> {
        let (parent, name) = parent(&j.backup_path)?;
        let root = self.root()?;
        if identity(&parent)?.dev != identity(&root)?.dev {
            return Err(RecoveryError::UnsafeRoot);
        }
        if stat(&parent, &name)?.is_none() {
            self.mkdir(j, &parent, &name)?;
        }
        let backup = directory(&parent, &name)?;
        let mut building = j.clone();
        building.backup_identity = Some(identity(&backup)?);
        self.sync(&building, &parent, "backup.parent_sync")?;
        let names_before = names(&backup)?;
        if names_before.iter().any(|n| {
            ![
                "data",
                "manifest.json",
                "manifest.json.pending",
                "previous-recovery.json",
            ]
            .contains(&n.as_str())
        }) {
            return Err(RecoveryError::BackupConflict);
        }
        if stat(&backup, "data")?.is_none() {
            if !names_before.is_empty() {
                return Err(RecoveryError::BackupConflict);
            }
            self.mkdir(&building, &backup, "data")?;
        }
        let data = directory(&backup, "data")?;
        building.data_identity = Some(identity(&data)?);
        self.sync(&building, &backup, "backup.directory_sync")?;
        if !names(&data)?.is_empty() {
            return Err(RecoveryError::BackupConflict);
        }
        let mut prepared = building;
        prepared.phase = JournalPhase::Prepared;
        let bytes = codec
            .encode(&prepared)
            .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
        for name in ["manifest.json", "manifest.json.pending"] {
            if let Some(saved) = read(&backup, name, JOURNAL_BOUND)?
                && codec
                    .decode(&saved)
                    .map_err(|_| RecoveryError::BackupConflict)?
                    != prepared
            {
                return Err(RecoveryError::BackupConflict);
            }
        }
        self.previous(&prepared, &backup)?;
        self.publish(
            &prepared,
            &backup,
            "manifest.json",
            "manifest.json.pending",
            &bytes,
            false,
        )?;
        let allowed: Vec<&str> = if j.previous_journal_bytes.is_some() {
            vec!["data", "manifest.json", "previous-recovery.json"]
        } else {
            vec!["data", "manifest.json"]
        };
        if names(&backup)?
            .iter()
            .any(|n| !allowed.contains(&n.as_str()))
        {
            return Err(RecoveryError::BackupConflict);
        }
        self.dirs(&prepared)?;
        Ok((identity(&backup)?, identity(&data)?))
    }
    fn previous(&self, j: &RecoveryJournal, backup: &OwnedFd) -> Result<(), RecoveryError> {
        let saved = read(backup, "previous-recovery.json", JOURNAL_BOUND)?;
        match (&j.previous_journal_bytes, saved) {
            (None, None) => Ok(()),
            (Some(expected), Some(actual)) if *expected == actual => {
                self.flush_existing(j, backup, "previous-recovery.json", expected)?;
                self.sync(j, backup, "previous.directory_sync")
            }
            (Some(expected), None) => {
                self.new_file(j, backup, "previous-recovery.json", expected)?;
                self.sync(j, backup, "previous.directory_sync")
            }
            _ => Err(RecoveryError::BackupConflict),
        }
    }
    fn mkdir(&self, j: &RecoveryJournal, dir: &OwnedFd, name: &str) -> Result<(), RecoveryError> {
        self.boundary(j, "directory.create")?;
        mkdirat(dir, name, Mode::from_bits_truncate(0o700)).map_err(|e| match e {
            Errno::EEXIST => RecoveryError::BackupConflict,
            _ => failure(e),
        })
    }
}

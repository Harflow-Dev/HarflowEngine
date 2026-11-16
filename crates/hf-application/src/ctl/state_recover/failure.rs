//! Qualified recovery failure evidence, retained from the admitted protocol only.
use super::{
    flow::RecoveryError,
    journal::{JournalPhase, RecoveryJournal},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryPhase {
    Planned,
    Prepared,
    BackedUp,
    Committed,
    Completed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryProgress {
    pub operation_id: String,
    pub backup_path: String,
    pub phase: RecoveryPhase,
}
impl RecoveryProgress {
    pub(super) fn journal(j: &RecoveryJournal) -> Self {
        Self {
            operation_id: j.operation_id.clone(),
            backup_path: j.backup_path.clone(),
            phase: match j.phase {
                JournalPhase::Planned => RecoveryPhase::Planned,
                JournalPhase::Prepared => RecoveryPhase::Prepared,
                JournalPhase::BackedUp => RecoveryPhase::BackedUp,
                JournalPhase::Completed => RecoveryPhase::Completed,
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryFailure {
    pub error: RecoveryError,
    pub progress: Option<RecoveryProgress>,
}
impl From<RecoveryError> for RecoveryFailure {
    fn from(error: RecoveryError) -> Self {
        Self {
            error,
            progress: None,
        }
    }
}
impl RecoveryFailure {
    pub(super) fn journal(error: RecoveryError, j: &RecoveryJournal) -> Self {
        Self {
            error,
            progress: Some(RecoveryProgress::journal(j)),
        }
    }
}

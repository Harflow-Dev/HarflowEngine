//! Precommit recovery journal values. These values perform no filesystem effects.
use super::evidence::{Basis, OwnerEvidence};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::{
        EntryBody, FileIdentity, INVENTORY_BOUND, INVENTORY_EXCLUDED, Inventory, RecoveryProposal,
    },
    state_recovery::is_operation_id,
};

pub const JOURNAL_BOUND: usize = 16 * 1024 * 1024;
pub const INVENTORY_DEPTH_BOUND: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalPhase {
    Planned,
    Prepared,
    BackedUp,
    Completed,
}
impl JournalPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Prepared => "prepared",
            Self::BackedUp => "backed_up",
            Self::Completed => "completed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    Invalid,
    UnsupportedPhase,
    UnsupportedExtension,
    TooLarge,
}

pub const FRESH_FILES: [&str; 4] = [
    "store.identity",
    "store.sqlite3",
    "store.sqlite3-wal",
    "store.watermark",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub identity: FileIdentity,
    pub size: u64,
    pub mtime_ns: i64,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryResult {
    pub operation_id: String,
    pub state_root: String,
    pub backup_path: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryJournal {
    pub engine_version: String,
    pub state_root: String,
    pub operation_id: String,
    pub proposal: RecoveryProposal,
    pub backup_path: String,
    pub root_identity: FileIdentity,
    pub lock_identity: FileIdentity,
    pub inventory: Inventory,
    pub created_at: String,
    pub phase: JournalPhase,
    pub backup_identity: Option<FileIdentity>,
    pub data_identity: Option<FileIdentity>,
    pub previous_journal_bytes: Option<Vec<u8>>,
    pub owner_evidence: OwnerEvidence,
    pub staging: Option<FileIdentity>,
    pub staged: Option<std::collections::BTreeMap<String, StagedFile>>,
    pub result: Option<RecoveryResult>,
}
impl RecoveryJournal {
    /// Structural cross-field checks, separate from current root admission.
    pub fn validate(&self) -> Result<(), JournalError> {
        let p = &self.proposal;
        let phase_ids = match self.phase {
            JournalPhase::Planned => self.backup_identity.is_none() && self.data_identity.is_none(),
            JournalPhase::Prepared | JournalPhase::BackedUp | JournalPhase::Completed => {
                self.backup_identity.is_some() && self.data_identity.is_some()
            }
        };
        if !is_operation_id(&self.operation_id)
            || self.engine_version != PRODUCT_VERSION
            || !is_digest(&p.inventory_sha256)
            || p.prior_journal_sha256
                .as_deref()
                .is_some_and(|v| !is_digest(v))
            || p.prior_journal_sha256.is_some() != self.previous_journal_bytes.is_some()
            || !self.state_root.starts_with('/')
            || self.state_root.contains('\0')
            || self.created_at.is_empty()
            || p.engine_version != self.engine_version
            || p.state_root != self.state_root
            || p.operation_id != self.operation_id
            || p.root_identity != self.root_identity
            || p.lock_identity
                .is_some_and(|lock| lock != self.lock_identity)
            || self.backup_path != p.backup_path()
            || self.inventory.root != self.root_identity
            || self.inventory.lock != Some(self.lock_identity)
            || self
                .inventory
                .entries
                .contains_key(&format!(".harflow-recovery-init-{}", self.operation_id))
            || self.owner_evidence.basis == Basis::Unreadable
            || !phase_ids
        {
            return Err(JournalError::Invalid);
        }
        let extension_valid = match self.phase {
            JournalPhase::Planned | JournalPhase::Prepared => {
                self.staging.is_none() && self.staged.is_none() && self.result.is_none()
            }
            JournalPhase::BackedUp => {
                self.result.is_none() && (self.staged.is_none() || self.staging.is_some())
            }
            JournalPhase::Completed => {
                self.result.is_some() && (self.staged.is_none() || self.staging.is_some())
            }
        };
        if !extension_valid {
            return Err(JournalError::Invalid);
        }
        if let Some(files) = &self.staged
            && (files.len() != FRESH_FILES.len()
                || FRESH_FILES.iter().any(|name| !files.contains_key(*name))
                || files.values().any(|file| !is_digest(&file.sha256)))
        {
            return Err(JournalError::Invalid);
        }
        if let Some(result) = &self.result
            && (result.operation_id != self.operation_id
                || result.state_root != self.state_root
                || result.backup_path != self.backup_path)
        {
            return Err(JournalError::Invalid);
        }
        validate_inventory(&self.inventory)?;
        Ok(())
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_inventory(inventory: &Inventory) -> Result<(), JournalError> {
    let mut stack = vec![(&inventory.entries, 0)];
    let mut count = 0usize;
    while let Some((children, depth)) = stack.pop() {
        if depth > INVENTORY_DEPTH_BOUND {
            return Err(JournalError::TooLarge);
        }
        for (name, entry) in children {
            count += 1;
            if count > INVENTORY_BOUND {
                return Err(JournalError::TooLarge);
            }
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains('/')
                || name.contains('\0')
                || (depth == 0 && INVENTORY_EXCLUDED.contains(&name.as_str()))
                || entry.stat.size < 0
            {
                return Err(JournalError::Invalid);
            }
            let mode = entry.stat.mode & 0o170000;
            match &entry.body {
                EntryBody::File if mode == 0o100000 => {}
                EntryBody::Link(target) if mode == 0o120000 && !target.contains('\0') => {}
                EntryBody::Directory(nested) if mode == 0o040000 => stack.push((nested, depth + 1)),
                _ => return Err(JournalError::Invalid),
            }
        }
    }
    Ok(())
}

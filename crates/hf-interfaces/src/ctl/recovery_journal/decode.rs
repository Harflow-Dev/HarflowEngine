use super::tree::{Tree, field, fields};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hf_application::ctl::state_recover::{
    evidence::{Basis, OwnerEvidence},
    journal::{JOURNAL_BOUND, JournalError, JournalPhase, RecoveryJournal},
};
use hf_domain::{
    ctl::recovery_proposal::{FileIdentity, Inventory, RecoveryProposal},
    process::ProcessIdentity,
};

/// Decode a complete supported precommit journal without recursive JSON parsing.
pub fn decode_journal(bytes: &[u8]) -> Result<RecoveryJournal, JournalError> {
    if bytes.len() > JOURNAL_BOUND {
        return Err(JournalError::TooLarge);
    }
    let t = Tree::parse(bytes)?;
    let m = t.object(0)?;
    fields(
        m,
        &[
            "product",
            "format",
            "v",
            "engine_version",
            "state_root",
            "operation_id",
            "proposal",
            "backup_path",
            "root_identity",
            "lock_identity",
            "inventory",
            "created_at",
            "phase",
            "backup_identity",
            "data_identity",
            "previous_journal_bytes",
            "owner_evidence",
        ],
    )?;
    let get = |key| field(m, key);
    if t.text(get("product")?)? != "harflow"
        || t.text(get("format")?)? != "harflow.state-recovery"
        || t.unsigned(get("v")?)? != 1
    {
        return Err(JournalError::Invalid);
    }
    let phase = match t.text(get("phase")?)?.as_str() {
        "planned" => JournalPhase::Planned,
        "prepared" => JournalPhase::Prepared,
        "backed_up" => JournalPhase::BackedUp,
        _ => return Err(JournalError::UnsupportedPhase),
    };
    let root_identity = identity(&t, get("root_identity")?)?;
    let lock_identity = identity(&t, get("lock_identity")?)?;
    let previous = get("previous_journal_bytes")?;
    let previous_journal_bytes = if t.null(previous) {
        None
    } else {
        Some(
            STANDARD
                .decode(t.text(previous)?)
                .map_err(|_| JournalError::Invalid)?,
        )
    };
    let journal = RecoveryJournal {
        engine_version: t.text(get("engine_version")?)?,
        state_root: t.text(get("state_root")?)?,
        operation_id: t.text(get("operation_id")?)?,
        proposal: proposal(&t, get("proposal")?)?,
        backup_path: t.text(get("backup_path")?)?,
        root_identity,
        lock_identity,
        inventory: Inventory {
            root: root_identity,
            lock: Some(lock_identity),
            entries: super::inventory_decode::decode(&t, get("inventory")?)?,
        },
        created_at: t.text(get("created_at")?)?,
        phase,
        backup_identity: optional_identity(&t, get("backup_identity")?)?,
        data_identity: optional_identity(&t, get("data_identity")?)?,
        previous_journal_bytes,
        owner_evidence: evidence(&t, get("owner_evidence")?)?,
    };
    journal.validate()?;
    Ok(journal)
}
pub(super) fn identity(t: &Tree, id: usize) -> Result<FileIdentity, JournalError> {
    let a = t.array(id)?;
    if a.len() != 2 {
        return Err(JournalError::Invalid);
    }
    Ok(FileIdentity {
        dev: t.signed(a[0])?,
        ino: t.unsigned(a[1])?,
    })
}
fn optional_identity(t: &Tree, id: usize) -> Result<Option<FileIdentity>, JournalError> {
    if t.null(id) {
        Ok(None)
    } else {
        identity(t, id).map(Some)
    }
}
fn proposal(t: &Tree, id: usize) -> Result<RecoveryProposal, JournalError> {
    let m = t.object(id)?;
    fields(
        m,
        &[
            "engine_version",
            "inventory_sha256",
            "lock_identity",
            "operation_id",
            "prior_journal_sha256",
            "root_identity",
            "state_root",
        ],
    )?;
    let get = |key| field(m, key);
    let prior = get("prior_journal_sha256")?;
    Ok(RecoveryProposal {
        engine_version: t.text(get("engine_version")?)?,
        inventory_sha256: t.text(get("inventory_sha256")?)?,
        lock_identity: optional_identity(t, get("lock_identity")?)?,
        operation_id: t.text(get("operation_id")?)?,
        prior_journal_sha256: if t.null(prior) {
            None
        } else {
            Some(t.text(prior)?)
        },
        root_identity: identity(t, get("root_identity")?)?,
        state_root: t.text(get("state_root")?)?,
    })
}
fn evidence(t: &Tree, id: usize) -> Result<OwnerEvidence, JournalError> {
    let m = t.object(id)?;
    fields(m, &["basis", "checked"])?;
    let basis = match t.text(field(m, "basis")?)?.as_str() {
        "clear" => Basis::Clear,
        "earlier_snapshot" => Basis::EarlierSnapshot,
        _ => return Err(JournalError::Invalid),
    };
    let mut checked = Vec::new();
    for id in t.array(field(m, "checked")?)? {
        let m = t.object(*id)?;
        fields(
            m,
            &["boot_uuid", "pid", "start_microseconds", "start_seconds"],
        )?;
        let pid =
            u32::try_from(t.unsigned(field(m, "pid")?)?).map_err(|_| JournalError::Invalid)?;
        checked.push(
            ProcessIdentity::new(
                pid,
                t.unsigned(field(m, "start_seconds")?)?,
                t.unsigned(field(m, "start_microseconds")?)?,
                t.text(field(m, "boot_uuid")?)?,
            )
            .map_err(|_| JournalError::Invalid)?,
        );
    }
    Ok(OwnerEvidence { basis, checked })
}

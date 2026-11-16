use super::*;
use crate::ctl::state_recover::{evidence, journal::JournalPhase};

pub(super) fn archive(
    mut j: RecoveryJournal,
    fresh: bool,
    p: &RecoveryPorts<'_>,
) -> Result<RecoveryJournal, RecoveryError> {
    archive_tracked(&mut j, fresh, p)?;
    Ok(j)
}
pub(super) fn archive_tracked(
    j: &mut RecoveryJournal,
    fresh: bool,
    p: &RecoveryPorts<'_>,
) -> Result<(), RecoveryError> {
    if fresh {
        // Even failure of the pending publication itself is an uncertain closed-gate
        // outcome: a retry must inspect it, never assume there was no mutation.
        p.mutations.write_pending(j).map_err(interrupted)?;
        publish_journal(j, p).map_err(interrupted)?;
    } else {
        // A parsed publication temporary is not proof that its file barrier
        // succeeded. Republish the admitted phase durably before moving data.
        publish_journal(j, p).map_err(interrupted)?;
    }
    archive_pending(j, p).map_err(interrupted)
}
fn archive_pending(j: &mut RecoveryJournal, p: &RecoveryPorts<'_>) -> Result<(), RecoveryError> {
    p.session.verify_held().map_err(admission::binding_error)?;
    if j.phase == JournalPhase::Planned {
        let (backup, data) = p.mutations.prepare_backup(j, p.journal_codec)?;
        j.backup_identity = Some(backup);
        j.data_identity = Some(data);
        j.phase = JournalPhase::Prepared;
        publish_journal(j, p)?;
    }
    p.mutations
        .validate_locations(j, j.phase == JournalPhase::BackedUp)?;
    evidence::recheck(&j.owner_evidence.checked, p.identities).map_err(admission::owner_error)?;
    if j.phase == JournalPhase::BackedUp {
        // R7 is never replayed from BackedUp. Reconcile only the same bounded
        // manifest, so an interrupted final manifest publication can finish.
        let bytes = admission::bounded(j, p)?;
        p.mutations.write_manifest(j, &bytes)?;
        return Ok(());
    }
    let mut names: Vec<_> = j.inventory.entries.keys().collect();
    names.sort_by_key(|name| (name.as_str() != "store.identity", name.as_str()));
    for name in names {
        p.session.verify_held().map_err(admission::binding_error)?;
        let expected = &j.inventory.entries[name];
        p.mutations.move_entry(j, name, expected)?;
    }
    p.mutations.move_shm(j)?;
    p.mutations.validate_locations(j, true)?;
    j.phase = JournalPhase::BackedUp;
    publish_journal(j, p)?;
    let bytes = admission::bounded(j, p)?;
    p.mutations.write_manifest(j, &bytes)?;
    p.mutations.validate_locations(j, true)
}
fn publish_journal(j: &RecoveryJournal, p: &RecoveryPorts<'_>) -> Result<(), RecoveryError> {
    let bytes = admission::bounded(j, p)?;
    p.mutations.write_journal(j, &bytes)
}
fn interrupted(e: RecoveryError) -> RecoveryError {
    match e {
        RecoveryError::InfrastructureUnavailable => RecoveryError::RecoveryInterrupted,
        other => other,
    }
}

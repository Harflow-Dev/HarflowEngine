use crate::{
    Error, Result, Store, ownership_codec as codec, ownership_read, ownership_schema,
    write_tx::{Publish, Stage},
};
use hf_domain::ownership::*;
use rusqlite::{Transaction, params};
use std::time::Instant;
pub(crate) fn transaction<T>(
    store: &Store,
    deadline: Instant,
    action: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<T> {
    store.write(
        deadline,
        Publish::Nothing,
        Some(Stage::OWNERSHIP),
        |transaction| {
            ownership_schema::check(transaction, &store.identity().generation)?;
            action(transaction)
        },
    )
}
pub(crate) fn generation(store: &Store, value: &Id) -> Result<()> {
    if value.as_str() != store.identity().generation {
        return Err(Error::StaleOwnership);
    }
    Ok(())
}
pub(crate) fn current(
    store: &Store,
    tx: &Transaction<'_>,
    operation: &Id,
    revision: u64,
) -> Result<Snapshot> {
    let snapshot = ownership_read::load(store, tx, operation)?.ok_or(Error::OwnershipConflict)?;
    if snapshot.revision != revision {
        return Err(Error::StaleOwnership);
    }
    Ok(snapshot)
}
pub(crate) fn fenced(snapshot: &Snapshot, fence: &Fence) -> Result<()> {
    if snapshot.fence().as_ref() != Some(fence) {
        return Err(Error::StaleOwnership);
    }
    Ok(())
}
pub(crate) fn persist(
    store: &Store,
    tx: &Transaction<'_>,
    snapshot: &Snapshot,
    prior: u64,
    proof: Option<&str>,
) -> Result<()> {
    let owner = snapshot.owner.as_ref();
    let bytes = snapshot.request.payload.len()
        + 248
        + owner.map_or(0, |v| 64 + codec::process(&v.process).len())
        + snapshot
            .child
            .as_ref()
            .map_or(0, |v| codec::process(v).len())
        + proof.map_or(0, str::len);
    store.check_bounds(1, bytes)?;
    let count = tx.execute("UPDATE ownership_records SET phase=?1,revision=?2,cancelled=?3,owner=?4,nonce=?5,owner_process=?6,child=?7,release_proof=?8 WHERE operation=?9 AND revision=?10", params![codec::phase_number(snapshot.phase),snapshot.revision as i64,snapshot.cancelled,owner.map(|v| v.owner.as_str()),owner.map(|v| v.nonce.as_str()),owner.map(|v| codec::process(&v.process)),snapshot.child.as_ref().map(codec::process),proof,snapshot.request.operation.as_str(),prior as i64])?;
    if count != 1 {
        return Err(Error::StaleOwnership);
    }
    Ok(())
}

use crate::{Error, Result, Store, conversation_codec as codec, ownership_read};
use hf_domain::{
    conversation::*,
    ownership::{Id, Phase},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
pub(crate) fn key(kind: &str, id: &Id) -> String {
    format!("__r06/{kind}/{}", id.as_str())
}
pub(crate) fn blob(store: &Store, tx: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    let meta: Option<(String, i64)> = tx
        .query_row(
            "SELECT typeof(value),length(value) FROM fixture_records WHERE key=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((kind, length)) = meta else {
        return Ok(None);
    };
    if kind != "blob" || length < 0 {
        return Err(Error::Incompatible);
    }
    let length = usize::try_from(length).map_err(|_| Error::LimitExceeded)?;
    if length > MAX_FRAME {
        return Err(Error::LimitExceeded);
    }
    store.check_bounds(1, length)?;
    Ok(Some(tx.query_row(
        "SELECT value FROM fixture_records WHERE key=?1",
        [key],
        |r| r.get(0),
    )?))
}
pub(crate) fn source(store: &Store, tx: &Connection, e: &Enrollment, active: bool) -> Result<()> {
    crate::ownership_write::generation(store, &e.source.generation)?;
    let snapshot =
        ownership_read::load(store, tx, &e.source.operation)?.ok_or(Error::OwnershipConflict)?;
    if snapshot.request != e.source
        || snapshot.owner.as_ref() != Some(&e.fence.owner)
        || snapshot.child.as_ref() != Some(&e.child)
        || snapshot.revision < e.fence.revision
        || !matches!(snapshot.phase, Phase::AcceptanceRecorded | Phase::Released)
    {
        return Err(Error::StaleOwnership);
    }
    if active
        && (snapshot.phase != Phase::AcceptanceRecorded
            || snapshot.cancelled
            || snapshot.fence().as_ref() != Some(&e.fence))
    {
        return Err(Error::StaleOwnership);
    }
    Ok(())
}
pub(crate) fn binding(store: &Store, e: &Enrollment) -> Result<Vec<u8>> {
    codec::encode(
        &store.identity.generation,
        &Ledger::new(e.clone()).map_err(Error::Conversation)?,
    )
}
pub(crate) fn indexes(store: &Store, tx: &Connection, e: &Enrollment) -> Result<()> {
    let expected = binding(store, e)?;
    for k in [
        key("source", &e.source.operation),
        key("context", &e.context),
    ] {
        let actual = blob(store, tx, &k)?.ok_or(Error::Incompatible)?;
        if actual != expected {
            return Err(Error::OwnershipConflict);
        }
    }
    Ok(())
}
pub(crate) fn load(store: &Store, tx: &Connection, id: &Id) -> Result<Ledger> {
    let bytes = blob(store, tx, &key("ledger", id))?
        .ok_or(Error::Conversation(hf_domain::conversation::Error::Missing))?;
    let ledger = codec::decode(&store.identity.generation, &bytes)?;
    if ledger.enrollment.conversation != *id {
        return Err(Error::Incompatible);
    }
    indexes(store, tx, &ledger.enrollment)?;
    source(store, tx, &ledger.enrollment, false)?;
    Ok(ledger)
}
pub(crate) fn persist(
    store: &Store,
    tx: &Transaction<'_>,
    ledger: &Ledger,
    admission: bool,
) -> Result<()> {
    let bytes = codec::encode(&store.identity.generation, ledger)?;
    let size = bytes
        .len()
        .checked_add(if admission { COMPLETION_RESERVE } else { 0 })
        .ok_or(Error::LimitExceeded)?;
    store.check_bounds(1, size)?;
    if size > MAX_FRAME {
        return Err(Error::LimitExceeded);
    }
    tx.execute("INSERT INTO fixture_records(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key("ledger",&ledger.enrollment.conversation),bytes])?;
    Ok(())
}

use crate::{Error, Result, Store, ownership_codec as codec};
use hf_domain::ownership::*;
use rusqlite::{Connection, OptionalExtension};
pub(crate) const PREFLIGHT_SQL: &str = "SELECT CASE WHEN typeof(operation)='text' AND length(CAST(operation AS BLOB))=32 AND typeof(session)='text' AND length(CAST(session AS BLOB))=32 AND typeof(reservation)='text' AND length(CAST(reservation AS BLOB))=32 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB))=32 AND typeof(generation)='text' AND length(CAST(generation AS BLOB))=32 AND typeof(digest)='text' AND length(CAST(digest AS BLOB))=64 AND typeof(payload)='blob' AND typeof(phase)='integer' AND typeof(revision)='integer' AND typeof(cancelled)='integer' AND (owner IS NULL OR (typeof(owner)='text' AND length(CAST(owner AS BLOB))=32)) AND (nonce IS NULL OR (typeof(nonce)='text' AND length(CAST(nonce AS BLOB))=32)) AND (owner_process IS NULL OR (typeof(owner_process)='text' AND length(CAST(owner_process AS BLOB))<=100)) AND (child IS NULL OR (typeof(child)='text' AND length(CAST(child AS BLOB))<=100)) AND (release_proof IS NULL OR (typeof(release_proof)='text' AND length(CAST(release_proof AS BLOB))<=2048)) THEN 1 ELSE 0 END,length(payload),length(payload)+248+coalesce(length(CAST(owner AS BLOB)),0)+coalesce(length(CAST(nonce AS BLOB)),0)+coalesce(length(CAST(owner_process AS BLOB)),0)+coalesce(length(CAST(child AS BLOB)),0)+coalesce(length(CAST(release_proof AS BLOB)),0) FROM ownership_records WHERE operation=?1";

pub(crate) fn load(
    store: &Store,
    connection: &Connection,
    operation: &Id,
) -> Result<Option<Snapshot>> {
    load_context(&store.read_context(), connection, operation)
}
pub(crate) fn load_context(
    store: &crate::read_context::ReadContext<'_>,
    connection: &Connection,
    operation: &Id,
) -> Result<Option<Snapshot>> {
    let lengths: Option<(i64, i64, i64)> = connection
        .query_row(PREFLIGHT_SQL, [operation.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((valid, length, bytes)) = lengths else {
        return Ok(None);
    };
    if valid != 1 || !(0..=16384).contains(&length) {
        return Err(Error::InvalidOwnership);
    }
    store.check_bounds(
        1,
        usize::try_from(bytes).map_err(|_| Error::InvalidOwnership)?,
    )?;
    let mut statement = connection.prepare("SELECT operation,session,reservation,attempt,generation,digest,payload,phase,revision,cancelled,owner,nonce,owner_process,child,release_proof FROM ownership_records WHERE operation=?1")?;
    let mut rows = statement.query([operation.as_str()])?;
    let row = rows.next()?.ok_or(Error::InvalidOwnership)?;
    let snapshot = decode(row)?;
    if snapshot.request.generation.as_str() != store.identity.generation {
        return Err(Error::InvalidOwnership);
    }
    codec::validate(&snapshot.request)?;
    Ok(Some(snapshot))
}
fn decode(row: &rusqlite::Row<'_>) -> Result<Snapshot> {
    let request = PreparedRequest {
        operation: codec::id(row.get(0)?)?,
        session: codec::id(row.get(1)?)?,
        reservation: codec::id(row.get(2)?)?,
        attempt: codec::id(row.get(3)?)?,
        generation: codec::id(row.get(4)?)?,
        digest: Digest::new(row.get(5)?).map_err(|_| Error::InvalidOwnership)?,
        payload: row.get(6)?,
    };
    let phase = codec::phase(row.get(7)?)?;
    let revision: i64 = row.get(8)?;
    let cancelled: i64 = row.get(9)?;
    if revision < 0 || !(0..=1).contains(&cancelled) {
        return Err(Error::InvalidOwnership);
    }
    let owner: Option<String> = row.get(10)?;
    let nonce: Option<String> = row.get(11)?;
    let physical: Option<String> = row.get(12)?;
    let owner = match (owner, nonce, physical) {
        (None, None, None) => None,
        (Some(owner), Some(nonce), Some(process)) => Some(OwnerClaim {
            owner: codec::id(owner)?,
            nonce: codec::id(nonce)?,
            process: codec::decode_process(&process)?,
        }),
        _ => return Err(Error::InvalidOwnership),
    };
    let child: Option<String> = row.get(13)?;
    let child = child.map(|v| codec::decode_process(&v)).transpose()?;
    let proof: Option<String> = row.get(14)?;
    let valid = match phase {
        Phase::Prepared => owner.is_none() && child.is_none() && proof.is_none(),
        Phase::Claimed => owner.is_some() && child.is_none() && proof.is_none(),
        Phase::SpawnObserved | Phase::AcceptanceRecorded => {
            owner.is_some() && child.is_some() && proof.is_none()
        }
        Phase::Released => {
            proof.as_ref().is_some_and(|v| !v.is_empty()) && (owner.is_some() || child.is_none())
        }
    };
    if !valid {
        return Err(Error::InvalidOwnership);
    }
    let snapshot = Snapshot {
        request,
        phase,
        revision: revision as u64,
        cancelled: cancelled == 1,
        owner,
        child,
    };
    if !codec::revision_valid(&snapshot) {
        return Err(Error::InvalidOwnership);
    }
    if let Some(proof) = proof {
        crate::ownership_release::validate(&snapshot, &proof)?;
    }
    Ok(snapshot)
}

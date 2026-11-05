use crate::{Error, Result, Store, capture_codec};
use hf_domain::{capture::*, ownership::Id, process::ProcessExit};
use rusqlite::{Connection, OptionalExtension};
pub(crate) fn load(store: &Store, db: &Connection, id: &Id) -> Result<Option<CaptureSnapshot>> {
    let valid: Option<(i64, i64)> = db.query_row("SELECT CASE WHEN typeof(binding)='text' AND length(CAST(binding AS BLOB)) BETWEEN 1 AND 512 AND typeof(operation)='text' AND length(CAST(operation AS BLOB))=32 AND typeof(revision)='integer' AND typeof(stdout)='integer' AND typeof(stderr)='integer' AND typeof(stdout_eof)='integer' AND typeof(stderr_eof)='integer' AND typeof(loss)='integer' AND typeof(exit_kind)='integer' AND typeof(exit_value)='integer' THEN 1 ELSE 0 END,length(CAST(binding AS BLOB)) FROM capture_records WHERE operation=?1", [id.as_str()], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    match valid {
        None => return Ok(None),
        Some((1, _)) => (),
        _ => return Err(Error::InvalidOwnership),
    }
    let bytes = valid.map(|v| v.1).ok_or(Error::InvalidOwnership)?;
    store.check_bounds(
        1,
        usize::try_from(bytes).map_err(|_| Error::InvalidOwnership)? + 104,
    )?;
    let row: (String, i64, i64, i64, i64, i64, i64, i64, i64) = db.query_row("SELECT binding,revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_records WHERE operation=?1", [id.as_str()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)))?;
    if row.1 < 0
        || !(0..=65536).contains(&row.2)
        || !(0..=65536).contains(&row.3)
        || [row.4, row.5, row.6].iter().any(|v| !(0..=1).contains(v))
    {
        return Err(Error::InvalidOwnership);
    }
    let exit = match (row.7, row.8) {
        (0, 0) => ProcessExit::Unobserved,
        (1, v) => ProcessExit::Code(i32::try_from(v).map_err(|_| Error::InvalidOwnership)?),
        (2, v) if v > 0 => {
            ProcessExit::Signal(i32::try_from(v).map_err(|_| Error::InvalidOwnership)?)
        }
        _ => return Err(Error::InvalidOwnership),
    };
    let binding = capture_codec::decode(&row.0)?;
    if binding.fence.operation != *id
        || binding.fence.generation.as_str() != store.identity().generation
        || (exit != ProcessExit::Unobserved && (row.4 != 1 || row.5 != 1 || row.6 != 0))
    {
        return Err(Error::InvalidOwnership);
    }
    Ok(Some(CaptureSnapshot {
        binding,
        progress: CaptureProgress {
            revision: row.1 as u64,
            stdout: row.2 as u64,
            stderr: row.3 as u64,
            stdout_eof: row.4 == 1,
            stderr_eof: row.5 == 1,
            loss: row.6 == 1,
        },
        exit,
    }))
}

use crate::{Error, Result, Store, lifecycle_codec as evidence, ownership_read};
use hf_domain::task_lifecycle::*;
use rusqlite::{Connection, OptionalExtension};
pub(crate) fn load(
    store: &Store,
    db: &Connection,
    binding: &LifecycleBinding,
) -> Result<Option<LifecycleEvidence>> {
    let valid:Option<bool>=db.query_row("SELECT typeof(task)='text' AND length(CAST(task AS BLOB)) BETWEEN 1 AND 64 AND typeof(operation)='text' AND length(CAST(operation AS BLOB))=32 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB))=32 AND typeof(generation)='text' AND length(CAST(generation AS BLOB))=32 AND typeof(revision)='integer' AND revision>=0 AND (cancel_seq IS NULL OR (typeof(cancel_seq)='integer' AND cancel_seq>0)) AND (cancel_reason IS NULL OR (typeof(cancel_reason)='text' AND length(CAST(cancel_reason AS BLOB))<=1024)) AND (cancel_at IS NULL OR (typeof(cancel_at)='text' AND length(CAST(cancel_at AS BLOB))<=128)) AND typeof(observation)='text' AND length(CAST(observation AS BLOB))<=16384 FROM lifecycle_records WHERE task=?1",[binding.task.as_str()],|r|r.get(0)).optional()?;
    if valid == Some(false) {
        return Err(Error::InvalidOwnership);
    }
    type StoredRow = (
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
    );
    let row:Option<StoredRow>=db.query_row("SELECT operation,attempt,generation,revision,cancel_seq,cancel_reason,cancel_at,observation FROM lifecycle_records WHERE task=?1",[binding.task.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
    let Some((operation, attempt, generation, revision, seq, reason, at, observation)) = row else {
        return Ok(None);
    };
    if operation != binding.operation.as_str()
        || attempt != binding.attempt.as_str()
        || generation != binding.generation.as_str()
        || revision < 0
    {
        return Err(Error::StaleOwnership);
    }
    let ownership =
        ownership_read::load(store, db, &binding.operation)?.ok_or(Error::InvalidOwnership)?;
    if ownership.request.attempt != binding.attempt
        || ownership.request.generation != binding.generation
    {
        return Err(Error::InvalidOwnership);
    }
    let cancel = match (seq, reason, at) {
        (None, None, None) => None,
        (Some(s), Some(reason), Some(accepted_at))
            if s > 0 && reason.len() <= 1024 && accepted_at.len() <= 128 =>
        {
            Some(CancelIntent {
                sequence: s as u64,
                reason,
                accepted_at,
            })
        }
        _ => return Err(Error::InvalidOwnership),
    };
    if ownership.cancelled != cancel.is_some() || ownership.revision > revision as u64 {
        return Err(Error::InvalidOwnership);
    }
    let decoded = evidence::decode(&observation)?;
    if evidence::encode(&decoded) != observation {
        return Err(Error::InvalidOwnership);
    }
    let observation = decoded;
    crate::lifecycle_invariants::validate_observation(&observation, &observation)?;
    Ok(Some(LifecycleEvidence {
        binding: binding.clone(),
        revision: revision as u64,
        ownership,
        cancel,
        observation,
    }))
}

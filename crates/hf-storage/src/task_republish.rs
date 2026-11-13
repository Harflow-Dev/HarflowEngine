//! Republishing a stored task's served header and status, with the recorded-status
//! monotonicity of the reference store: a terminal status is never replaced by a
//! non-terminal one.
use crate::{Error, Result, Store, task_writer::BLOB_LIMIT, write_tx::Publish};
use hf_domain::{
    ctl::caller::TERMINAL_STATUSES,
    task_read::{StoredObservation, TaskId},
};
use rusqlite::{OptionalExtension, Transaction};
use std::time::Instant;

/// Whether a republish landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Republished {
    /// The rows were replaced and the change published.
    Landed,
    /// The stored status is terminal and the new one is not: nothing was written,
    /// published or committed.
    KeptTerminal,
}

impl Store {
    /// Replaces the served header and status blobs of a valid task, and the observation
    /// columns readers filter on, in one commit that rotates the dataset generation and
    /// bumps the watch revision. A missing or damaged task is refused with `TaskMissing`.
    /// The task's facts row is left unchanged.
    pub fn republish_header(
        &self,
        id: &TaskId,
        header: &[u8],
        status: &[u8],
        observation: &StoredObservation,
        deadline: Instant,
    ) -> Result<Republished> {
        if header.len() > BLOB_LIMIT
            || status.len() > BLOB_LIMIT
            || observation
                .recorded_status
                .as_ref()
                .is_some_and(|s| s.len() > 256)
        {
            return Err(Error::LimitExceeded);
        }
        let landed = self.write_unless(deadline, Publish::GenerationAndRevision, None, |tx| {
            let recorded = recorded_status(tx, id)?;
            if is_terminal(recorded.as_deref())
                && !is_terminal(observation.recorded_status.as_deref())
            {
                return Ok(None);
            }
            replace(tx, id, header, status, observation)?;
            Ok(Some(()))
        })?;
        Ok(match landed {
            Some(()) => Republished::Landed,
            None => Republished::KeptTerminal,
        })
    }
}

pub(crate) fn is_terminal(status: Option<&str>) -> bool {
    status.is_some_and(|status| TERMINAL_STATUSES.contains(&status))
}
/// The recorded status of a valid task with a header row: the header's observation column,
/// else the facts row's status.
pub(crate) fn recorded_status(tx: &Transaction<'_>, id: &TaskId) -> Result<Option<String>> {
    let found: Option<(Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT h.recorded_status, f.status FROM task_facts f JOIN task_headers h ON h.id=f.id WHERE f.id=?1 AND f.valid=1",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (header, facts) = found.ok_or(Error::TaskMissing)?;
    Ok(header.or(facts))
}
fn replace(
    tx: &Transaction<'_>,
    id: &TaskId,
    header: &[u8],
    status: &[u8],
    observation: &StoredObservation,
) -> Result<()> {
    tx.execute(
        "UPDATE task_headers SET active=?2,recorded_status=?3,exited=?4,drained=?5,owned=?6,header=?7,status=?8 WHERE id=?1",
        rusqlite::params![
            id.as_str(),
            observation.active,
            observation.recorded_status,
            observation.exited,
            observation.drained,
            observation.owned,
            header,
            status
        ],
    )?;
    Ok(())
}

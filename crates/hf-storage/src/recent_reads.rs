//! Reads that serve `status`: the recent event bodies of a brief status and the
//! workflow-builder fact of a task.
use crate::{Error, Result, event_read, snapshot::TaskSnapshot, statements, task_catalog::within};
use hf_domain::task_read::TaskId;
use rusqlite::OptionalExtension;

/// The stored limit of one event body.
const BODY_BYTES: usize = 16_384;
/// The log's tail a recent-activity read walks: logical bytes of the complete log, and
/// the same figure again as real body bytes kept.
const RECENT_WINDOW: u64 = 1_048_576;

impl TaskSnapshot<'_> {
    /// Stored event bodies from the last megabyte of the complete log, oldest first. The
    /// walk stops once the bodies kept would pass the window, and leaves out a body over
    /// the stored limit or with no content. What bounds the walk is the logical window,
    /// as in the reference: every record spans at least one byte of it, and the deadline is
    /// checked at every row. A bad stream row gives no bodies, as an unreadable log does;
    /// store, busy and deadline failures are errors.
    pub fn recent_event_bodies(&self, id: &TaskId) -> Result<Vec<Vec<u8>>> {
        self.step()?;
        let stream = match event_read::stream_optional(self.db, id) {
            Ok(Some(stream)) if stream.facts.available => stream,
            Ok(_) | Err(Error::EventReadUnavailable) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let end = i64::try_from(stream.complete_end).map_err(|_| Error::LimitExceeded)?;
        let from = i64::try_from(stream.complete_end.saturating_sub(RECENT_WINDOW))
            .map_err(|_| Error::LimitExceeded)?;
        let mut statement = self.db.prepare(statements::recent_bodies())?;
        let mut rows = statement.query(rusqlite::params![id.as_str(), from, end])?;
        let mut bodies = Vec::new();
        let mut kept = 0u64;
        while let Some(row) = rows.next()? {
            self.step()?;
            let length = row
                .get::<_, Option<i64>>(0)?
                .and_then(|n| u64::try_from(n).ok());
            let Some(length) = length.filter(|length| *length > 0) else {
                continue;
            };
            if kept + length > RECENT_WINDOW {
                break;
            }
            if let Some(body) = row.get::<_, Option<Vec<u8>>>(1)?
                && body.len() <= BODY_BYTES
            {
                kept += length;
                bodies.push(body);
            }
        }
        bodies.reverse();
        Ok(bodies)
    }

    /// Whether the task's header or status blob says it is a workflow builder. Neither blob
    /// is parsed unless both together fit the session's query byte budget; past it the
    /// read is `LimitExceeded`, as for an over-budget listing, never "not a builder".
    pub fn workflow_builder(&self, id: &TaskId) -> Result<bool> {
        self.step()?;
        let lengths: Option<(Option<i64>, i64)> = self
            .db
            .query_row(statements::blob_lengths(), [id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((header, status)) = lengths else {
            return Ok(false);
        };
        let bytes = within(header, i64::MAX)? + within(Some(status), i64::MAX)?;
        self.context.check_bounds(1, bytes)?;
        self.step()?;
        let found: Option<i64> = self
            .db
            .query_row(&statements::workflow_builder(), [id.as_str()], |r| r.get(0))
            .optional()?;
        Ok(found == Some(1))
    }
}

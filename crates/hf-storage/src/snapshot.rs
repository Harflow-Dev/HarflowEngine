//! The named queries of one read transaction for the ctl task reads. Nothing here leaves
//! storage as a raw connection: callers get typed rows, each query inside the session
//! limits, all from the one snapshot.
use crate::{
    Error, Result, connection,
    read_context::ReadContext,
    statements,
    task_catalog::{task_id, within},
    task_query, task_read,
};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    ctl::task_rows::{TaskRecord, TaskSummary},
    task_read::{AuthorizedScope, Lookup, TaskId},
};
use rusqlite::Connection;
use std::time::Instant;

pub struct TaskSnapshot<'a> {
    pub(crate) context: &'a ReadContext<'a>,
    pub(crate) db: &'a Connection,
    pub(crate) deadline: Instant,
}

impl<'a> TaskSnapshot<'a> {
    pub(crate) fn new(context: &'a ReadContext<'a>, db: &'a Connection, deadline: Instant) -> Self {
        Self {
            context,
            db,
            deadline,
        }
    }

    pub(crate) fn step(&self) -> Result<()> {
        connection::timeout(self.db, self.deadline)
    }

    /// Whether the history cannot be trusted as a whole: any damaged task record, root-wide,
    /// or any incomplete dataset coverage flag. A store with no dataset row is an error.
    pub fn damaged(&self) -> Result<bool> {
        self.step()?;
        let (_, coverage) =
            task_read::dataset_optional(self.db)?.ok_or(Error::TaskReadUnavailable)?;
        if !(coverage.discovery_complete && coverage.ownership_complete && coverage.source_complete)
        {
            return Ok(true);
        }
        self.step()?;
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_facts WHERE valid=0)",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn lookup(&self, id: &TaskId) -> Result<Lookup> {
        self.step()?;
        let (_, coverage) =
            task_read::dataset_optional(self.db)?.ok_or(Error::TaskReadUnavailable)?;
        task_query::lookup(
            self.db,
            &AuthorizedScope::Global,
            id,
            coverage,
            self.deadline,
        )
    }

    /// The title is a display projection, cut in SQL, so one long title cannot break a
    /// prefix diagnostic.
    pub fn summaries(&self, ids: &[TaskId]) -> Result<Vec<TaskSummary>> {
        self.context.check_bounds(ids.len(), 0)?;
        let mut found = Vec::new();
        let mut bytes = 0usize;
        for id in ids {
            self.step()?;
            let mut statement = self.db.prepare(statements::summary())?;
            let mut rows = statement.query([id.as_str()])?;
            let Some(row) = rows.next()? else { continue };
            let sizes = [within(row.get(4)?, 256)?, within(row.get(5)?, 64)?];
            let title: Option<String> = row.get(1)?;
            bytes += sizes.iter().sum::<usize>() + title.as_ref().map_or(0, String::len) + 64;
            self.context.check_bounds(found.len() + 1, bytes)?;
            found.push(TaskSummary {
                id: task_id(row.get(0)?)?,
                title,
                status: row.get(2)?,
                created_at: row.get(3)?,
            });
        }
        Ok(found)
    }

    pub fn record(&self, id: &TaskId) -> Result<Option<TaskRecord>> {
        let mut found = self.select(
            &statements::record(),
            vec![rusqlite::types::Value::Text(id.as_str().to_owned())],
            1,
        )?;
        Ok(found.pop())
    }

    /// The digest a page cursor is bound to: the store generation, the filters that shape the
    /// result and nothing about the dataset or the page size.
    pub fn cursor_scope(
        &self,
        active_only: bool,
        session: Option<&str>,
        predecessor: Option<&TaskId>,
    ) -> Result<String> {
        let part = |text: Option<&str>| match text {
            Some(text) => format!("{}:{text}", text.len()).into_bytes(),
            None => b"-".to_vec(),
        };
        let parts = [
            b"harflow.ctl.cursor.v1".to_vec(),
            part(Some(&self.context.identity.generation)),
            part(Some(if active_only { "1" } else { "0" })),
            part(session),
            part(predecessor.map(TaskId::as_str)),
        ];
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        Ok(crate::digest::Sha256Digest
            .digest_parts(&refs)
            .as_str()
            .to_owned())
    }
}

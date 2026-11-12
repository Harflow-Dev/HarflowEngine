//! Pre-record launch attempts (schema 7): one row per receipt filename stem. Receipts are
//! display evidence, never task status; an ordinary record with the same id wins.
//!
//! Row classes, enforced by the table's CHECK constraints:
//! * an invalid name (not a task identity): `task_id` and every other column NULL;
//! * a refused receipt: both refusal columns set and the display columns NULL. Its status
//!   columns (`state`, `not_started_reason`, `has_resources`, `has_worktree`) are set only when the receipt still
//!   qualifies as a launch document, so the ordinary reads can see it;
//! * otherwise a complete launch whose `resource_names` is a JSON array and NULL without
//!   resources.
use crate::{Error, Result, Store, connection};
use hf_domain::task_read::{PendingLaunch, TaskId};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use std::time::Instant;

pub(crate) const SCHEMA: &str = "
CREATE TABLE launch_attempts(
  name TEXT PRIMARY KEY NOT NULL CHECK(typeof(name)='text' AND length(name)>0),
  task_id TEXT CHECK(task_id IS NULL OR (typeof(task_id)='text' AND task_id=name
    AND instr(CAST(task_id AS BLOB), x'00')=0
    AND length(task_id) BETWEEN 1 AND 64 AND task_id GLOB '[A-Za-z0-9]*'
    AND task_id NOT GLOB '*[^A-Za-z0-9_-]*')),
  generation TEXT, state TEXT, title TEXT, backend TEXT, repo_path TEXT, created_at TEXT,
  not_started_reason TEXT, spawned_by TEXT, root_task_id TEXT, parent_task_id TEXT,
  chain_root_task_id TEXT, resource_names TEXT,
  has_resources INTEGER CHECK(has_resources IS NULL OR has_resources IN(0,1)),
  has_worktree INTEGER CHECK(has_worktree IS NULL OR has_worktree IN(0,1)),
  refusal_code TEXT, refusal_check TEXT,
  CHECK(state IS NULL OR state IN('reserved','claimed','waiting_for_resources','granted','spawned','settled','not_started')),
  CHECK((refusal_code IS NULL)=(refusal_check IS NULL)),
  CHECK((state IS NULL)=(has_resources IS NULL) AND (state IS NULL)=(has_worktree IS NULL)),
  CHECK(resource_names IS NULL OR (has_resources=1 AND typeof(resource_names)='text'
    AND CASE WHEN json_valid(resource_names) THEN json_type(resource_names)='array' ELSE 0 END)),
  CHECK(task_id IS NOT NULL OR (generation IS NULL AND state IS NULL AND title IS NULL
    AND backend IS NULL AND repo_path IS NULL AND created_at IS NULL AND not_started_reason IS NULL
    AND spawned_by IS NULL AND root_task_id IS NULL AND parent_task_id IS NULL
    AND chain_root_task_id IS NULL AND resource_names IS NULL AND refusal_code IS NULL)),
  CHECK(refusal_code IS NULL OR (task_id IS NOT NULL AND generation IS NULL AND title IS NULL
    AND backend IS NULL AND repo_path IS NULL AND created_at IS NULL AND spawned_by IS NULL
    AND root_task_id IS NULL AND parent_task_id IS NULL AND chain_root_task_id IS NULL
    AND resource_names IS NULL)),
  CHECK(task_id IS NULL OR refusal_code IS NOT NULL
    OR (generation IS NOT NULL AND state IS NOT NULL AND backend IS NOT NULL))
);
";
/// The statement the ordinary reads use: only an unspawned, unsettled launch that asked for
/// resources or a worktree has no record yet because its owner holds the wait.
const PENDING_KIND: &str = "SELECT state,not_started_reason FROM launch_attempts \
    WHERE name=?1 AND task_id IS NOT NULL AND state IS NOT NULL \
    AND state NOT IN('spawned','settled') AND (has_resources=1 OR has_worktree=1)";

/// One launch row as the fixture writers store it. Column classes are documented on the
/// module; the table refuses a row that mixes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchRow {
    pub name: String,
    pub task_id: Option<TaskId>,
    pub generation: Option<String>,
    pub state: Option<String>,
    pub title: Option<String>,
    pub backend: Option<String>,
    pub repo_path: Option<String>,
    pub created_at: Option<String>,
    pub not_started_reason: Option<String>,
    pub spawned_by: Option<String>,
    pub root_task_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub chain_root_task_id: Option<String>,
    pub resource_names: Option<Vec<String>>,
    pub has_resources: Option<bool>,
    pub has_worktree: Option<bool>,
    pub refusal_code: Option<String>,
    pub refusal_check: Option<String>,
}
impl LaunchRow {
    /// A pending-kind launch of `id` waiting for a resource: the shape the ordinary reads
    /// turn into `task_pending_launch`.
    pub fn waiting_for_resources(id: TaskId) -> Self {
        Self {
            name: id.as_str().to_owned(),
            task_id: Some(id),
            generation: Some("c".repeat(32)),
            state: Some("waiting_for_resources".into()),
            backend: Some("claude".into()),
            resource_names: Some(vec!["fixture".into()]),
            has_resources: Some(true),
            has_worktree: Some(false),
            ..Self::default()
        }
    }
    /// The bytes this row adds to a write, shared by every writer that stores launch rows.
    pub(crate) fn bytes(&self) -> usize {
        let text = |value: &Option<String>| value.as_ref().map_or(0, String::len);
        self.name.len()
            + text(&self.generation)
            + text(&self.state)
            + text(&self.title)
            + text(&self.backend)
            + text(&self.repo_path)
            + text(&self.created_at)
            + text(&self.not_started_reason)
            + text(&self.spawned_by)
            + text(&self.root_task_id)
            + text(&self.parent_task_id)
            + text(&self.chain_root_task_id)
            + text(&self.refusal_code)
            + text(&self.refusal_check)
            + self
                .resource_names
                .as_ref()
                .map_or(0, |names| names.iter().map(|name| name.len() + 4).sum())
    }
}
/// Resource names as stored: a compact JSON array of strings.
fn names_text(names: &[String]) -> String {
    let mut text = String::from("[");
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push('"');
        for ch in name.chars() {
            match ch {
                '"' => text.push_str("\\\""),
                '\\' => text.push_str("\\\\"),
                c if (c as u32) < 0x20 => text.push_str(&format!("\\u{:04x}", c as u32)),
                c => text.push(c),
            }
        }
        text.push('"');
    }
    text.push(']');
    text
}
pub(crate) fn insert(tx: &Transaction<'_>, row: &LaunchRow, replace: bool) -> Result<()> {
    let verb = if replace {
        "INSERT OR REPLACE"
    } else {
        "INSERT"
    };
    tx.execute(
        &format!("{verb} INTO launch_attempts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)"),
        rusqlite::params![
            row.name,
            row.task_id.as_ref().map(TaskId::as_str),
            row.generation,
            row.state,
            row.title,
            row.backend,
            row.repo_path,
            row.created_at,
            row.not_started_reason,
            row.spawned_by,
            row.root_task_id,
            row.parent_task_id,
            row.chain_root_task_id,
            row.resource_names.as_deref().map(names_text),
            row.has_resources,
            row.has_worktree,
            row.refusal_code,
            row.refusal_check
        ],
    )?;
    Ok(())
}
/// The pending-kind launch of `id`, when its receipt makes the id a task that has not
/// started.
pub(crate) fn pending(db: &rusqlite::Connection, id: &TaskId) -> Result<Option<PendingLaunch>> {
    let found: Option<(String, Option<String>)> = db
        .query_row(PENDING_KIND, [id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(found.map(|(state, reason)| PendingLaunch {
        id: id.clone(),
        state,
        not_started_reason: reason.filter(|reason| !reason.is_empty()),
    }))
}
/// The refusal of a receipt that does not qualify as a launch document: the ordinary reads
/// cannot say what the task is.
pub(crate) fn refused(db: &rusqlite::Connection, id: &TaskId) -> Result<Option<(String, String)>> {
    Ok(db
        .query_row(
            "SELECT refusal_code,refusal_check FROM launch_attempts \
             WHERE name=?1 AND task_id IS NOT NULL AND state IS NULL AND refusal_code IS NOT NULL",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}
fn advance_paging_generation(tx: &Transaction<'_>) -> Result<()> {
    crate::task_read::dataset_optional(tx)?.ok_or(Error::TaskReadUnavailable)?;
    tx.execute(
        "UPDATE task_dataset SET generation=lower(hex(randomblob(16))) WHERE singleton=1",
        [],
    )?;
    Ok(())
}
impl Store {
    /// Launch-only fixture update: replaces the whole set of launch rows and advances the
    /// paging generation, but never task-list metadata. A launch may share its id with an
    /// ordinary record; duplicate names refuse atomically.
    pub fn replace_launch_fixture(&self, rows: &[LaunchRow], deadline: Instant) -> Result<()> {
        self.check_bounds(rows.len(), 0)?;
        let mut names = std::collections::BTreeSet::new();
        let mut bytes = 0usize;
        for row in rows {
            connection::check_deadline(deadline)?;
            bytes = bytes.checked_add(row.bytes()).ok_or(Error::LimitExceeded)?;
            self.check_bounds(rows.len(), bytes)?;
            if !names.insert(row.name.as_str()) {
                return Err(Error::Incompatible);
            }
        }
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io(deadline)?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        advance_paging_generation(&transaction)?;
        transaction.execute("DELETE FROM launch_attempts", [])?;
        for row in rows {
            connection::timeout(&transaction, deadline)?;
            insert(&transaction, row, false)?;
        }
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)
    }
    /// Only this row's own bounds are checked here. Namespace totals (entries and name
    /// bytes) are enforced at read time, through `PendingLimits`.
    ///
    /// Adds a launch row, or replaces the content of the row with the same name in place.
    /// The paging generation advances; task-list metadata does not change.
    pub fn put_launch_attempt(&self, row: &LaunchRow, deadline: Instant) -> Result<()> {
        self.check_bounds(1, row.bytes())?;
        self.fixture_write(deadline, |tx| {
            advance_paging_generation(tx)?;
            insert(tx, row, true)
        })
    }
}

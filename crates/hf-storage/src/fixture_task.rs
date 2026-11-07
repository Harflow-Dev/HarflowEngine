//! Per-task fixture writers: the typed facts, the served blobs, the prompt record and the
//! optional caller-scan process row are written together in one transaction.
use crate::{Error, Result, Store};
use hf_domain::{process::ProcessIdentity, task_read::TaskId};
use rusqlite::{OptionalExtension, Transaction};
use std::time::Instant;
const BLOB_LIMIT: usize = 2048;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureFacts {
    pub id: TaskId,
    pub backend: String,
    pub repo_path: String,
    pub session_id: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub started_at_us: i64,
    pub finished_at: Option<String>,
    pub finished_at_us: Option<i64>,
    pub output_drained: bool,
    pub title: Option<String>,
    pub model: Option<String>,
    pub continuation_of: Option<TaskId>,
    pub chain_root: Option<TaskId>,
}
/// `inline_text` is set exactly when `inline`; otherwise a preview, a source name and,
/// optionally, a claimed digest and the stored full text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixturePrompt {
    pub inline_text: Option<String>,
    pub preview: Option<String>,
    pub claimed_sha256: Option<String>,
    pub source_name: Option<String>,
    pub blob: Option<Vec<u8>>,
}
/// `start` is `None` when the identity cannot be decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureProcess {
    pub pid: u32,
    pub start: Option<ProcessIdentity>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureTask {
    pub facts: FixtureFacts,
    pub header: Vec<u8>,
    pub status: Vec<u8>,
    pub prompt: FixturePrompt,
    pub process: Option<FixtureProcess>,
}
impl Store {
    /// Inserts one valid task. An existing id is refused; nothing is replaced.
    pub fn add_task_fixture(&self, task: &FixtureTask, deadline: Instant) -> Result<()> {
        validate(task)?;
        self.check_bounds(1, payload_bytes(task))?;
        self.fixture_write(deadline, |tx| {
            if exists(tx, &task.facts.id)? {
                return Err(Error::FixtureConflict);
            }
            insert(tx, task)?;
            crate::fixture_tx::task_changed(tx)
        })
    }
    /// Replaces the task's metadata with a damaged record: the header and process rows go and
    /// the facts row becomes `valid=0` with every fact NULL. Prompt, artifact and event rows
    /// stay, as the Python port keeps those files. Appending an event to a damaged task is
    /// refused, because its stream facts have no source.
    pub fn damage_task_fixture(&self, id: &TaskId, deadline: Instant) -> Result<()> {
        self.fixture_write(deadline, |tx| {
            for table in ["task_headers WHERE id", "task_processes WHERE task"] {
                tx.execute(&format!("DELETE FROM {table}=?1"), [id.as_str()])?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO task_facts(id,valid) VALUES(?1,0)",
                [id.as_str()],
            )?;
            crate::fixture_tx::task_changed(tx)
        })
    }
}
fn validate(task: &FixtureTask) -> Result<()> {
    let facts = &task.facts;
    if task.header.len() > BLOB_LIMIT
        || task.status.len() > BLOB_LIMIT
        || facts.status.len() > 256
        || facts.session_id.len() > 1024
        || facts.session_id.chars().take(257).count() > 256
        || facts.finished_at.is_some() != facts.finished_at_us.is_some()
        || task
            .process
            .as_ref()
            .is_some_and(|p| p.pid <= 1 || p.pid > i32::MAX as u32)
    {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
fn payload_bytes(task: &FixtureTask) -> usize {
    let p = &task.prompt;
    p.inline_text.as_ref().map_or(0, String::len)
        + p.preview.as_ref().map_or(0, String::len)
        + p.blob.as_ref().map_or(0, Vec::len)
        + task.header.len()
        + task.status.len()
}
/// `None` when no row exists, otherwise whether the record is a valid task.
pub(crate) fn task_valid(tx: &Transaction<'_>, id: &TaskId) -> Result<Option<bool>> {
    Ok(tx
        .query_row(
            "SELECT valid FROM task_facts WHERE id=?1",
            [id.as_str()],
            |r| r.get::<_, bool>(0),
        )
        .optional()?)
}
fn exists(tx: &Transaction<'_>, id: &TaskId) -> Result<bool> {
    Ok(task_valid(tx, id)?.is_some())
}
fn insert(tx: &Transaction<'_>, task: &FixtureTask) -> Result<()> {
    let f = &task.facts;
    tx.execute(
        "INSERT INTO task_facts VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        rusqlite::params![
            f.id.as_str(),
            f.backend,
            f.repo_path,
            f.session_id,
            f.status,
            f.exit_code,
            f.started_at,
            f.started_at_us,
            f.finished_at,
            f.finished_at_us,
            f.output_drained,
            f.title,
            f.model,
            f.continuation_of.as_ref().map(TaskId::as_str),
            f.chain_root.as_ref().map(TaskId::as_str)
        ],
    )?;
    tx.execute(
        "INSERT INTO task_headers VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9)",
        rusqlite::params![
            f.id.as_str(),
            f.started_at_us,
            f.session_id,
            f.status == "running",
            f.status,
            f.exit_code.is_some(),
            f.output_drained,
            task.header,
            task.status
        ],
    )?;
    insert_prompt(tx, f.id.as_str(), &task.prompt)?;
    if let Some(process) = &task.process {
        insert_process(tx, f.id.as_str(), process)?;
    }
    Ok(())
}
fn insert_prompt(tx: &Transaction<'_>, id: &str, prompt: &FixturePrompt) -> Result<()> {
    let inline = prompt.inline_text.is_some();
    tx.execute(
        "INSERT INTO task_prompts VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            id,
            if inline { "inline" } else { "sidecar" },
            prompt.inline_text,
            prompt.preview,
            prompt.claimed_sha256,
            prompt.source_name,
            !inline,
            prompt.blob
        ],
    )?;
    Ok(())
}
fn insert_process(tx: &Transaction<'_>, id: &str, process: &FixtureProcess) -> Result<()> {
    let start = process.start.as_ref();
    tx.execute(
        "INSERT INTO task_processes VALUES(?1,?2,?3,?4,?5,NULL)",
        rusqlite::params![
            id,
            process.pid,
            start.and_then(|s| i64::try_from(s.start_seconds()).ok()),
            start.map(|s| s.start_microseconds() as i64),
            start.map(|s| s.boot_uuid().to_owned())
        ],
    )?;
    Ok(())
}

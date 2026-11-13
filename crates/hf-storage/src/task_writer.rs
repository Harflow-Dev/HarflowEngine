//! The production task writer: one transaction stores the typed facts, the served header and
//! status blobs, the prompt record and the optional caller-scan process row, then rotates the
//! dataset generation and bumps the watch revision.
use crate::{Error, Result, Store, history_reads::PROMPT_VERIFY_BYTES, write_tx::Publish};
use hf_domain::{ownership::Digest, process::ProcessIdentity, task_read::TaskId};
use rusqlite::{OptionalExtension, Transaction};
use sha2::{Digest as _, Sha256};
use std::time::Instant;

/// Largest header or status blob, in bytes.
pub(crate) const BLOB_LIMIT: usize = 2048;
/// A prompt of more characters than this is stored as a sidecar with a preview of this many.
pub const PROMPT_PREVIEW_CHARS: usize = 2000;
/// Largest prompt the writer stores, so that every stored sidecar can be verified on read.
pub const PROMPT_BLOB_LIMIT: usize = 4 * 1024 * 1024;
const _: () = assert!(PROMPT_BLOB_LIMIT as i64 == PROMPT_VERIFY_BYTES);
/// Largest `backend`, `model`, `started_at`, `finished_at` or `chain_root` text, in bytes.
pub const FACT_TEXT_LIMIT: usize = 2048;
/// Largest `repo_path`, in bytes: a path the platforms accept (Linux `PATH_MAX`).
pub const REPO_PATH_LIMIT: usize = 4096;
/// Largest title, in bytes. The prompt read measures inline text or preview, title and
/// source name together against the read budget; the bound keeps every written task within
/// the default budget (a 2000-character preview is at most 8000 bytes; a source name is at
/// most a 64-byte id, `.prompt.`, 64 hex digits and `.txt`).
pub const TITLE_LIMIT: usize = 65_536;
const SOURCE_NAME_LIMIT: usize = 64 + ".prompt.".len() + 64 + ".txt".len();
const _: () =
    assert!(PROMPT_PREVIEW_CHARS * 4 + TITLE_LIMIT + SOURCE_NAME_LIMIT <= crate::DEFAULT_MAX_BYTES);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFacts {
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
    /// The stored text, verbatim; it need not be a task identity.
    pub chain_root: Option<String>,
}
/// `start` is `None` when the identity cannot be decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskProcess {
    pub pid: u32,
    pub start: Option<ProcessIdentity>,
}
/// A new task. `header` and `status` are the served bytes, at most 2048 each; the prompt is
/// the full text, stored inline up to 2000 characters and as a sidecar blob above that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskCreate {
    pub facts: TaskFacts,
    pub header: Vec<u8>,
    pub status: Vec<u8>,
    pub prompt: String,
    pub process: Option<TaskProcess>,
}
/// One `task_prompts` row: `inline_text` is set exactly for the inline form.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PromptRow<'a> {
    pub inline_text: Option<&'a str>,
    pub preview: Option<&'a str>,
    pub claimed_sha256: Option<&'a str>,
    pub source_name: Option<&'a str>,
    pub blob: Option<&'a [u8]>,
}
/// The rows of one new task, as the production writer and the fixture wrapper hand them over.
pub(crate) struct NewTask<'a> {
    pub facts: &'a TaskFacts,
    pub header: &'a [u8],
    pub status: &'a [u8],
    pub prompt: PromptRow<'a>,
    pub process: Option<&'a TaskProcess>,
}

impl Store {
    /// Stores one new task and publishes it. An existing id, valid or damaged, is refused
    /// with `TaskExists` and nothing is written.
    /// Refused with `LimitExceeded` before any write: a blob over 2048 bytes, a prompt over
    /// 4 MiB, a fact text over its bound, or a process whose identity names another pid.
    pub fn create_task(&self, task: &TaskCreate, deadline: Instant) -> Result<()> {
        if task.prompt.len() > PROMPT_BLOB_LIMIT || !facts_within(task) {
            return Err(Error::LimitExceeded);
        }
        let id = task.facts.id.as_str();
        let cut = task.prompt.char_indices().nth(PROMPT_PREVIEW_CHARS);
        let sidecar = cut.map(|(end, _)| {
            let digest = sha256_hex(task.prompt.as_bytes());
            let name = format!("{id}.prompt.{digest}.txt");
            (end, digest, name)
        });
        let prompt = match &sidecar {
            None => PromptRow {
                inline_text: Some(&task.prompt),
                preview: None,
                claimed_sha256: None,
                source_name: None,
                blob: None,
            },
            Some((end, digest, name)) => PromptRow {
                inline_text: None,
                preview: Some(&task.prompt[..*end]),
                claimed_sha256: Some(digest),
                source_name: Some(name),
                blob: Some(task.prompt.as_bytes()),
            },
        };
        self.insert_task(
            &NewTask {
                facts: &task.facts,
                header: &task.header,
                status: &task.status,
                prompt,
                process: task.process.as_ref(),
            },
            deadline,
        )
    }
    /// The shared insert of `create_task` and the fixture wrapper: validation, then one
    /// commit that rotates the dataset generation and bumps the watch revision.
    pub(crate) fn insert_task(&self, task: &NewTask<'_>, deadline: Instant) -> Result<()> {
        validate(task)?;
        self.write(deadline, Publish::GenerationAndRevision, None, |tx| {
            if task_valid(tx, &task.facts.id)?.is_some() {
                return Err(Error::TaskExists);
            }
            insert(tx, task)
        })
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Digest::from_sha256_bytes(Sha256::digest(bytes).into())
        .as_str()
        .to_owned()
}
/// The production bounds the fixture wrapper does not apply: fixtures seed over-long facts
/// and mismatched identities to exercise the read bounds.
fn facts_within(task: &TaskCreate) -> bool {
    let f = &task.facts;
    let short = |text: Option<&str>| text.is_none_or(|t| t.len() <= FACT_TEXT_LIMIT);
    f.repo_path.len() <= REPO_PATH_LIMIT
        && f.title.as_ref().is_none_or(|t| t.len() <= TITLE_LIMIT)
        && [
            Some(f.backend.as_str()),
            f.model.as_deref(),
            Some(f.started_at.as_str()),
            f.finished_at.as_deref(),
            f.chain_root.as_deref(),
        ]
        .into_iter()
        .all(short)
        && task
            .process
            .as_ref()
            .is_none_or(|p| p.start.as_ref().is_none_or(|start| start.pid() == p.pid))
}
fn validate(task: &NewTask<'_>) -> Result<()> {
    let facts = task.facts;
    if task.header.len() > BLOB_LIMIT
        || task.status.len() > BLOB_LIMIT
        || facts.status.len() > 256
        || facts.session_id.len() > 1024
        || facts.session_id.chars().take(257).count() > 256
        || facts.finished_at.is_some() != facts.finished_at_us.is_some()
        || task
            .process
            .is_some_and(|p| p.pid <= 1 || p.pid > i32::MAX as u32 || !start_fits(p))
    {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
/// The start seconds are stored as a SQLite integer.
fn start_fits(process: &TaskProcess) -> bool {
    process
        .start
        .as_ref()
        .is_none_or(|start| i64::try_from(start.start_seconds()).is_ok())
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
fn insert(tx: &Transaction<'_>, task: &NewTask<'_>) -> Result<()> {
    let f = task.facts;
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
            f.chain_root
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
    if let Some(process) = task.process {
        insert_process(tx, f.id.as_str(), process)?;
    }
    Ok(())
}
fn insert_prompt(tx: &Transaction<'_>, id: &str, prompt: &PromptRow<'_>) -> Result<()> {
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
fn insert_process(tx: &Transaction<'_>, id: &str, process: &TaskProcess) -> Result<()> {
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

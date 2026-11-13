//! Per-task fixture writers. Adding a task is a thin wrapper over the production insert
//! (`task_writer`); damaging one stays fixture-only.
use crate::{
    Error, Result, Store,
    task_writer::{NewTask, PromptRow, TaskFacts, TaskProcess},
    write_tx::Publish,
};
use hf_domain::task_read::TaskId;
use rusqlite::OptionalExtension;
use std::time::Instant;
pub type FixtureFacts = TaskFacts;
pub type FixtureProcess = TaskProcess;
/// `inline_text` is set exactly when `inline`; otherwise a preview, a source name and,
/// optionally, a claimed digest and the stored full text. Unlike `create_task`, the row is
/// stored as given, so fixtures can seed damaged and over-long prompts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixturePrompt {
    pub inline_text: Option<String>,
    pub preview: Option<String>,
    pub claimed_sha256: Option<String>,
    pub source_name: Option<String>,
    pub blob: Option<Vec<u8>>,
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
    /// Inserts one valid task through the production insert, with the prompt row as given
    /// and the store's `Limits` as an extra bound. An existing id is refused; nothing is
    /// replaced.
    pub fn add_task_fixture(&self, task: &FixtureTask, deadline: Instant) -> Result<()> {
        self.check_bounds(1, payload_bytes(task))?;
        let p = &task.prompt;
        let new = NewTask {
            facts: &task.facts,
            header: &task.header,
            status: &task.status,
            prompt: PromptRow {
                inline_text: p.inline_text.as_deref(),
                preview: p.preview.as_deref(),
                claimed_sha256: p.claimed_sha256.as_deref(),
                source_name: p.source_name.as_deref(),
                blob: p.blob.as_deref(),
            },
            process: task.process.as_ref(),
        };
        self.insert_task(&new, deadline)
            .map_err(|error| match error {
                Error::TaskExists => Error::FixtureConflict,
                other => other,
            })
    }
    /// The chain root stored for `id`: the outer `None` means the task is missing or damaged,
    /// the inner one that it stores none. Used to resolve a continuation's default chain root.
    pub fn fixture_chain_root(
        &self,
        id: &TaskId,
        deadline: Instant,
    ) -> Result<Option<Option<String>>> {
        let connection = self.admit(deadline)?;
        let found: Option<Option<String>> = connection
            .query_row(
                "SELECT chain_root FROM task_facts WHERE id=?1 AND valid=1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        self.binding.check()?;
        Ok(found)
    }
    /// Replaces the task's metadata with a damaged record: the header and process rows go and
    /// the facts row becomes `valid=0` with every fact NULL. Prompt, artifact and event rows
    /// stay, as the Python port keeps those files. Appending an event to a damaged task is
    /// refused, because its stream facts have no source.
    pub fn damage_task_fixture(&self, id: &TaskId, deadline: Instant) -> Result<()> {
        self.write(deadline, Publish::GenerationAndRevision, None, |tx| {
            for table in ["task_headers WHERE id", "task_processes WHERE task"] {
                tx.execute(&format!("DELETE FROM {table}=?1"), [id.as_str()])?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO task_facts(id,valid) VALUES(?1,0)",
                [id.as_str()],
            )?;
            Ok(())
        })
    }
}
fn payload_bytes(task: &FixtureTask) -> usize {
    let p = &task.prompt;
    p.inline_text.as_ref().map_or(0, String::len)
        + p.preview.as_ref().map_or(0, String::len)
        + p.blob.as_ref().map_or(0, Vec::len)
        + task.header.len()
        + task.status.len()
}

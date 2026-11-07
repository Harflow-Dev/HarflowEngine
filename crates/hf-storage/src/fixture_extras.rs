//! Prompt blob and artifact manifest fixture writers.
use crate::{Error, Result, Store, fixture_task};
use hf_domain::task_read::TaskId;
use rusqlite::OptionalExtension;
use std::time::Instant;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureArtifact {
    pub artifact_id: String,
    pub path: String,
    pub kind: String,
    pub note: String,
    pub registered_at: String,
    pub provenance_method: String,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: u64,
}
impl Store {
    /// Appends `suffix` to the stored full prompt, leaving the claimed digest untouched.
    /// Returns whether the task had a stored full prompt; without one nothing changes.
    pub fn append_prompt_blob_fixture(
        &self,
        id: &TaskId,
        suffix: &[u8],
        deadline: Instant,
    ) -> Result<bool> {
        self.check_bounds(1, suffix.len())?;
        self.fixture_write(deadline, |tx| {
            let current: Option<Option<Vec<u8>>> = tx
                .query_row(
                    "SELECT blob FROM task_prompts WHERE task=?1",
                    [id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(Some(mut blob)) = current else {
                return Ok(false);
            };
            blob.extend_from_slice(suffix);
            self.check_bounds(1, blob.len())?;
            tx.execute(
                "UPDATE task_prompts SET blob=?2 WHERE task=?1",
                rusqlite::params![id.as_str(), blob],
            )?;
            Ok(true)
        })
    }
    /// Replaces the whole manifest of a valid task; a new revision binds artifact cursors.
    pub fn replace_artifacts_fixture(
        &self,
        id: &TaskId,
        header_valid: bool,
        items: &[FixtureArtifact],
        deadline: Instant,
    ) -> Result<()> {
        let bytes = items
            .iter()
            .map(|i| i.path.len() + i.note.len() + 128)
            .sum();
        self.check_bounds(items.len(), bytes)?;
        if items.iter().any(|i| i.size > i64::MAX as u64) {
            return Err(Error::LimitExceeded);
        }
        self.fixture_write(deadline, |tx| {
            if fixture_task::task_valid(tx, id)? != Some(true) {
                return Err(Error::FixtureMissing);
            }
            tx.execute("DELETE FROM artifact_items WHERE task=?1", [id.as_str()])?;
            tx.execute(
                "INSERT INTO artifact_manifests VALUES(?1,?2,lower(hex(randomblob(16)))) ON CONFLICT(task) DO UPDATE SET header_valid=excluded.header_valid,revision=excluded.revision",
                rusqlite::params![id.as_str(), header_valid],
            )?;
            for (ordinal, item) in items.iter().enumerate() {
                insert_item(tx, id, ordinal, item)?;
            }
            Ok(())
        })
    }
}
fn insert_item(
    tx: &rusqlite::Transaction<'_>,
    id: &TaskId,
    ordinal: usize,
    item: &FixtureArtifact,
) -> Result<()> {
    tx.execute(
        "INSERT INTO artifact_items VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        rusqlite::params![
            id.as_str(),
            ordinal as i64,
            item.artifact_id,
            item.path,
            item.kind,
            item.note,
            item.registered_at,
            item.provenance_method,
            item.device.to_string(),
            item.inode.to_string(),
            item.size as i64,
            item.mtime_ns.to_string()
        ],
    )?;
    Ok(())
}

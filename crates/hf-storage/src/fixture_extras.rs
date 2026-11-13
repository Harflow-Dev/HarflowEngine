//! Prompt blob and artifact manifest fixture writers.
use crate::{
    Error, Result, Store,
    artifact_writer::{self, ItemRow},
    task_writer,
};
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
    /// Required for `observed_backend_plan`, absent for every other method.
    pub plan: Option<FixturePlan>,
    /// The item's bytes in the reference manifest document (the interface layer's codec);
    /// stored as given, at most `i64::MAX`.
    pub encoded_len: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixturePlan {
    pub tool_use_id: String,
    /// The recorded directory and its device and inode.
    pub directory: Option<(String, u64, u64)>,
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
    /// Items go through the production insert without its duplicate, cap or size checks.
    pub fn replace_artifacts_fixture(
        &self,
        id: &TaskId,
        header_valid: bool,
        items: &[FixtureArtifact],
        deadline: Instant,
    ) -> Result<()> {
        let bytes = items
            .iter()
            .map(|i| i.path.len() + i.note.len() + 128 + plan_bytes(i))
            .sum();
        self.check_bounds(items.len(), bytes)?;
        if items
            .iter()
            .any(|i| i.size > i64::MAX as u64 || i.encoded_len > i64::MAX as u64)
        {
            return Err(Error::LimitExceeded);
        }
        self.fixture_write(deadline, |tx| {
            if task_writer::task_valid(tx, id)? != Some(true) {
                return Err(Error::FixtureMissing);
            }
            tx.execute("DELETE FROM artifact_items WHERE task=?1", [id.as_str()])?;
            artifact_writer::upsert_manifest(tx, id, header_valid)?;
            for (ordinal, item) in items.iter().enumerate() {
                let ordinal = i64::try_from(ordinal).map_err(|_| Error::LimitExceeded)?;
                artifact_writer::insert_item(tx, id, ordinal, &row(item)?)?;
            }
            Ok(())
        })
    }
}
fn row(item: &FixtureArtifact) -> Result<ItemRow<'_>> {
    let directory = item.plan.as_ref().and_then(|p| p.directory.as_ref());
    Ok(ItemRow {
        artifact_id: &item.artifact_id,
        path: &item.path,
        kind: &item.kind,
        note: &item.note,
        registered_at: &item.registered_at,
        method: &item.provenance_method,
        device: item.device.to_string(),
        inode: item.inode.to_string(),
        size: i64::try_from(item.size).map_err(|_| Error::LimitExceeded)?,
        mtime_ns: item.mtime_ns.to_string(),
        tool_use_id: item.plan.as_ref().map(|p| p.tool_use_id.as_str()),
        directory: directory.map(|d| (d.0.as_str(), d.1, d.2)),
        encoded_len: i64::try_from(item.encoded_len).map_err(|_| Error::LimitExceeded)?,
    })
}
fn plan_bytes(item: &FixtureArtifact) -> usize {
    item.plan.as_ref().map_or(0, |p| {
        p.tool_use_id.len() + p.directory.as_ref().map_or(0, |d| d.0.len())
    })
}

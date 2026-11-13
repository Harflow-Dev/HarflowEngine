//! The production artifact manifest writer: registers one metadata-only reference to a valid
//! task's manifest in one commit. The manifest revision (which binds artifact cursors) rotates;
//! nothing is published to the task list.
//!
//! Each stored item carries its encoded length: the bytes of the item in the reference
//! manifest document (`json.dumps(..., ensure_ascii=False)`), computed by the interface layer's
//! codec and handed over by the caller, since this crate does not encode JSON. The serialized
//! manifest is then measured inside the transaction as the caller's envelope length (the
//! document with an empty `items` list), plus the stored and new item lengths summed in SQL,
//! plus the two-byte item separator between consecutive items.
use crate::{Error, Result, Store, task_republish, task_writer::task_valid, write_tx::Publish};
use hf_domain::{
    ctl::history::{ArtifactItem, MANIFEST_ITEMS_MAX, PlanDirectory, PlanEvidence},
    task_read::TaskId,
};
use rusqlite::{OptionalExtension, Transaction};
use std::time::Instant;

/// Most items one manifest holds (reference `artifacts.py`, the 100-item cap).
pub const ARTIFACT_ITEMS_LIMIT: usize = MANIFEST_ITEMS_MAX;
/// Largest serialized manifest, in bytes (reference `artifacts.py` `MAX_BYTES`).
pub const MANIFEST_BYTES_LIMIT: u64 = 1_048_576;
/// Largest `path` or `note`, in bytes; the reads refuse a longer one.
pub const ARTIFACT_TEXT_LIMIT: usize = 4096;
/// Largest `artifact_id`, `kind`, `registered_at` and `provenance.method` together, in bytes.
pub const ARTIFACT_LABELS_LIMIT: usize = 1024;
/// The provenance method whose items carry plan evidence, and only those.
const PLAN_METHOD: &str = "observed_backend_plan";
/// The bytes `json.dumps` writes between two items of the `items` list (`", "`).
const ITEM_SEPARATOR: u64 = 2;

/// The encoded lengths of one registration, from the interface layer's manifest codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestLengths {
    /// The manifest document of this task with an empty `items` list.
    pub envelope: u64,
    /// The new item as the manifest stores it.
    pub item: u64,
}

/// Who registers, which decides the reference's liveness and duplicate rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registrant {
    /// A verified task process (`register`): refused once the task's recorded status is
    /// terminal; a duplicate is the same path, kind, note and observation.
    Caller,
    /// The engine's own bookkeeping (engine output after the task finished, observed backend
    /// plans): any valid task; a duplicate is the same path and observation.
    Engine,
}

/// Whether a registration wrote anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Registered {
    /// The item was appended and the manifest revision rotated.
    Added,
    /// An equal reference is already stored: this one, first in manifest order. Nothing was
    /// written, published or committed.
    Existing(Box<ArtifactItem>),
}

/// One `artifact_items` row as the writer and the fixture wrapper hand it over. The
/// observation's device, inode and mtime are stored as decimal text.
pub(crate) struct ItemRow<'a> {
    pub artifact_id: &'a str,
    pub path: &'a str,
    pub kind: &'a str,
    pub note: &'a str,
    pub registered_at: &'a str,
    pub method: &'a str,
    pub device: String,
    pub inode: String,
    pub size: i64,
    pub mtime_ns: String,
    pub tool_use_id: Option<&'a str>,
    pub directory: Option<(&'a str, u64, u64)>,
    pub encoded_len: i64,
}

impl Store {
    /// Appends `item` to the manifest of a valid task in one commit that rotates the manifest
    /// revision and publishes nothing else (no dataset generation or watch revision change).
    /// A task without a manifest gets one. A duplicate (see `Registrant`) returns the stored
    /// item without a commit.
    ///
    /// Refused before the transaction with `LimitExceeded`: a `path` or `note` over 4096
    /// bytes, labels over 1024 bytes together, a `tool_use_id` outside 1..=256 bytes, a plan
    /// directory outside 1..=4096 bytes, a size over `i64::MAX`, or lengths that alone exceed
    /// the manifest bound. Refused inside it, with nothing committed: `TaskMissing` for a
    /// missing or damaged task, or (`Caller`) a task without a header row; `TaskFinished` for a
    /// `Caller` registration to a task whose recorded status is terminal; `Incompatible` for a
    /// manifest whose header is invalid; `LimitExceeded` for a manifest already holding 100
    /// items or a serialized manifest that would exceed 1,048,576 bytes. Checks run in that
    /// order, with the duplicate lookup between the header check and the cap. A `plan` present
    /// for any method other than `observed_backend_plan`, or absent for it, is also refused
    /// before the transaction with `LimitExceeded` (the writer's shape refusal).
    pub fn register_artifact(
        &self,
        id: &TaskId,
        item: &ArtifactItem,
        lengths: ManifestLengths,
        registrant: Registrant,
        deadline: Instant,
    ) -> Result<Registered> {
        let row = item_row(item, lengths)?;
        let mut existing = None;
        let added = self.write_unless(deadline, Publish::Nothing, None, |tx| {
            admit(tx, id, registrant)?;
            existing = duplicate(tx, id, item, registrant)?;
            if existing.is_some() {
                return Ok(None);
            }
            let ordinal = within_bounds(tx, id, lengths)?;
            upsert_manifest(tx, id, true)?;
            insert_item(tx, id, ordinal, &row)?;
            Ok(Some(()))
        })?;
        match (added, existing) {
            (Some(()), _) => Ok(Registered::Added),
            (None, Some(found)) => Ok(Registered::Existing(Box::new(found))),
            // `write_unless` rolls back only when the action found a duplicate.
            (None, None) => Err(Error::Incompatible),
        }
    }
}

fn item_row(item: &ArtifactItem, lengths: ManifestLengths) -> Result<ItemRow<'_>> {
    let labels =
        item.artifact_id.len() + item.kind.len() + item.registered_at.len() + item.method.len();
    let plan = item.plan.as_ref();
    let directory = plan.and_then(|p| p.directory.as_ref());
    if item.path.len() > ARTIFACT_TEXT_LIMIT
        || item.note.len() > ARTIFACT_TEXT_LIMIT
        || labels > ARTIFACT_LABELS_LIMIT
        || plan.is_some_and(|p| !(1..=256).contains(&p.tool_use_id.len()))
        || directory.is_some_and(|d| !(1..=ARTIFACT_TEXT_LIMIT).contains(&d.path.len()))
        || lengths.envelope.saturating_add(lengths.item) > MANIFEST_BYTES_LIMIT
        || (item.method == PLAN_METHOD) != plan.is_some()
    {
        return Err(Error::LimitExceeded);
    }
    Ok(ItemRow {
        artifact_id: &item.artifact_id,
        path: &item.path,
        kind: &item.kind,
        note: &item.note,
        registered_at: &item.registered_at,
        method: &item.method,
        device: item.registered.device.to_string(),
        inode: item.registered.inode.to_string(),
        size: i64::try_from(item.registered.size).map_err(|_| Error::LimitExceeded)?,
        mtime_ns: item.registered.mtime_ns.to_string(),
        tool_use_id: plan.map(|p| p.tool_use_id.as_str()),
        directory: directory.map(|d| (d.path.as_str(), d.device, d.inode)),
        encoded_len: i64::try_from(lengths.item).map_err(|_| Error::LimitExceeded)?,
    })
}

/// The task must be valid; a caller's task must also not be terminal.
fn admit(tx: &Transaction<'_>, id: &TaskId, registrant: Registrant) -> Result<()> {
    match registrant {
        Registrant::Caller => {
            if task_republish::is_terminal(task_republish::recorded_status(tx, id)?.as_deref()) {
                return Err(Error::TaskFinished);
            }
        }
        Registrant::Engine => {
            if task_valid(tx, id)? != Some(true) {
                return Err(Error::TaskMissing);
            }
        }
    }
    let header: Option<bool> = tx
        .query_row(
            "SELECT header_valid FROM artifact_manifests WHERE task=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if header == Some(false) {
        return Err(Error::Incompatible);
    }
    Ok(())
}

/// The first stored item equal to `item` under the registrant's duplicate rule.
fn duplicate(
    tx: &Transaction<'_>,
    id: &TaskId,
    item: &ArtifactItem,
    registrant: Registrant,
) -> Result<Option<ArtifactItem>> {
    let o = &item.registered;
    let size = i64::try_from(o.size).map_err(|_| Error::LimitExceeded)?;
    let observed = format!(
        "SELECT artifact_id,registered_at,provenance_method,tool_use_id,plan_directory,plan_device,plan_inode,kind,note \
         FROM artifact_items WHERE task=?1 AND path=?2 AND device=?3 AND inode=?4 AND size=?5 AND mtime_ns=?6{} \
         ORDER BY ordinal LIMIT 1",
        match registrant {
            Registrant::Caller => " AND kind=?7 AND note=?8",
            Registrant::Engine => "",
        }
    );
    let (device, inode, mtime_ns) = (
        o.device.to_string(),
        o.inode.to_string(),
        o.mtime_ns.to_string(),
    );
    let base: [&dyn rusqlite::ToSql; 6] =
        [&id.as_str(), &item.path, &device, &inode, &size, &mtime_ns];
    let labels: [&dyn rusqlite::ToSql; 2] = [&item.kind, &item.note];
    let params: Vec<&dyn rusqlite::ToSql> = match registrant {
        Registrant::Caller => base.iter().chain(labels.iter()).copied().collect(),
        Registrant::Engine => base.to_vec(),
    };
    let found = tx
        .query_row(&observed, params.as_slice(), |r| {
            Ok(Stored {
                artifact_id: r.get(0)?,
                registered_at: r.get(1)?,
                method: r.get(2)?,
                tool_use_id: r.get(3)?,
                directory: (r.get(4)?, r.get(5)?, r.get(6)?),
                kind: r.get(7)?,
                note: r.get(8)?,
            })
        })
        .optional()?;
    found.map(|stored| stored.into_item(item)).transpose()
}

struct Stored {
    artifact_id: String,
    registered_at: String,
    method: String,
    tool_use_id: Option<String>,
    directory: (Option<String>, Option<String>, Option<String>),
    kind: String,
    note: String,
}
impl Stored {
    /// The stored item; its path and observation equal the request's.
    fn into_item(self, request: &ArtifactItem) -> Result<ArtifactItem> {
        let number = |text: String| text.parse::<u64>().map_err(|_| Error::MalformedRow);
        let directory = match self.directory {
            (None, None, None) => None,
            (Some(path), Some(device), Some(inode)) => Some(PlanDirectory {
                path,
                device: number(device)?,
                inode: number(inode)?,
            }),
            _ => return Err(Error::MalformedRow),
        };
        let plan = self.tool_use_id.map(|tool_use_id| PlanEvidence {
            tool_use_id,
            directory,
        });
        Ok(ArtifactItem {
            artifact_id: self.artifact_id,
            path: request.path.clone(),
            kind: self.kind,
            note: self.note,
            registered_at: self.registered_at,
            method: self.method,
            registered: request.registered,
            plan,
        })
    }
}

/// The cap and the serialized bound for one more item; returns the new item's ordinal.
fn within_bounds(tx: &Transaction<'_>, id: &TaskId, lengths: ManifestLengths) -> Result<i64> {
    let (count, stored, next): (i64, i64, i64) = tx.query_row(
        "SELECT count(*),coalesce(sum(encoded_len),0),coalesce(max(ordinal)+1,0) FROM artifact_items WHERE task=?1",
        [id.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let count = u64::try_from(count).map_err(|_| Error::MalformedRow)?;
    let stored = u64::try_from(stored).map_err(|_| Error::MalformedRow)?;
    if count >= ARTIFACT_ITEMS_LIMIT as u64 {
        return Err(Error::LimitExceeded);
    }
    // `count` stored items plus the new one: `count` separators.
    let total = [
        lengths.envelope,
        stored,
        lengths.item,
        ITEM_SEPARATOR * count,
    ]
    .into_iter()
    .try_fold(0u64, u64::checked_add)
    .ok_or(Error::LimitExceeded)?;
    if total > MANIFEST_BYTES_LIMIT {
        return Err(Error::LimitExceeded);
    }
    Ok(next)
}

/// Gives the task's manifest a fresh revision, creating it with a valid header when absent.
pub(crate) fn upsert_manifest(tx: &Transaction<'_>, id: &TaskId, header_valid: bool) -> Result<()> {
    tx.execute(
        "INSERT INTO artifact_manifests VALUES(?1,?2,lower(hex(randomblob(16)))) ON CONFLICT(task) DO UPDATE SET header_valid=excluded.header_valid,revision=excluded.revision",
        rusqlite::params![id.as_str(), header_valid],
    )?;
    Ok(())
}

/// The insert primitive shared with the fixture wrapper; no bound or duplicate check.
pub(crate) fn insert_item(
    tx: &Transaction<'_>,
    id: &TaskId,
    ordinal: i64,
    row: &ItemRow<'_>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO artifact_items VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        rusqlite::params![
            id.as_str(),
            ordinal,
            row.artifact_id,
            row.path,
            row.kind,
            row.note,
            row.registered_at,
            row.method,
            row.device,
            row.inode,
            row.size,
            row.mtime_ns,
            row.tool_use_id,
            row.directory.map(|d| d.0),
            row.directory.map(|d| d.1.to_string()),
            row.directory.map(|d| d.2.to_string()),
            row.encoded_len,
        ],
    )?;
    Ok(())
}

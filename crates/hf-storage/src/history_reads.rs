//! The history reads of the ctl snapshot: event pages, the change token, prompts, artifact
//! manifests and continuation links. Everything is bounded by the session limits except
//! the one named prompt read; recent event bodies live in `recent_reads`.
use crate::{
    Error, Result,
    digest::Sha256Digest,
    event_read,
    snapshot::TaskSnapshot,
    statements,
    task_catalog::{task_id, within},
};
use hf_application::{event_read::EventReadResult, owner_runtime::DigestPort};
use hf_domain::{
    ctl::{
        history::sidecar_digest,
        history::{ArtifactItem, LogObject, ManifestHead, Observation, PromptBlob, StoredPrompt},
        task_rows::TaskRecord,
    },
    event_read::EventQuery,
    task_read::TaskId,
};
use rusqlite::OptionalExtension;

/// A sidecar up to this size is read whole and hashed; the one read of a ctl snapshot that
/// can exceed the query budget, because rusqlite offers no incremental blob read here.
pub const PROMPT_VERIFY_BYTES: i64 = 4 * 1024 * 1024;

fn part(parts: &mut Vec<Vec<u8>>, bytes: &[u8]) {
    let mut framed = bytes.len().to_string().into_bytes();
    framed.push(b':');
    framed.extend_from_slice(bytes);
    parts.push(framed);
}

fn optional(parts: &mut Vec<Vec<u8>>, text: Option<&str>) {
    match text {
        Some(text) => part(parts, text.as_bytes()),
        None => parts.push(b"-".to_vec()),
    }
}

/// The record fields, the recorded process and the blobs a token binds, in a fixed order.
fn fingerprint(parts: &mut Vec<Vec<u8>>, record: &TaskRecord) {
    let facts = &record.facts;
    let numbers = [
        facts.exit_code,
        Some(facts.started_at_us),
        facts.finished_at_us,
    ];
    for text in [
        facts.id.as_str(),
        &facts.backend,
        &facts.repo_path,
        &facts.session_id,
        &facts.status,
        &facts.started_at,
    ] {
        part(parts, text.as_bytes());
    }
    for number in numbers {
        optional(parts, number.map(|n| n.to_string()).as_deref());
    }
    optional(parts, facts.finished_at.as_deref());
    part(parts, &[u8::from(facts.output_drained)]);
    optional(parts, facts.title.as_deref());
    optional(parts, facts.model.as_deref());
    optional(parts, facts.continuation_of.as_ref().map(TaskId::as_str));
    optional(parts, facts.chain_root.raw());
    match &record.process {
        Some(process) => {
            part(parts, &process.pid.to_le_bytes());
            match &process.start {
                Some(start) => {
                    part(parts, &start.start_seconds().to_le_bytes());
                    part(parts, &start.start_microseconds().to_le_bytes());
                    part(parts, start.boot_uuid().as_bytes());
                }
                None => parts.push(b"-".to_vec()),
            }
        }
        None => parts.push(b"-".to_vec()),
    }
    part(parts, &record.header);
    match &record.status_blob {
        Some(blob) => part(parts, blob),
        None => parts.push(b"-".to_vec()),
    }
}

fn hex_digest(parts: &[Vec<u8>]) -> String {
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    Sha256Digest.digest_parts(&refs).as_str().to_owned()
}

fn unsigned(text: &str) -> Result<u64> {
    text.parse().map_err(|_| Error::MalformedRow)
}

impl TaskSnapshot<'_> {
    /// One page of the task's event log, read in this snapshot. A cursor of a generation
    /// the store no longer serves is `ExpiredEventCursor`.
    pub fn event_page(&self, query: &EventQuery) -> Result<EventReadResult> {
        query.validate().map_err(|_| Error::InvalidEventQuery)?;
        self.step()?;
        event_read::run(self.context, self.db, query, self.deadline)
    }

    /// Hex SHA-256 over the cursor generation, the task record, the store and stream
    /// generations, the log's extent and the computed log object.
    pub fn event_change_token(&self, record: &TaskRecord, log: &LogObject) -> Result<String> {
        self.step()?;
        let mut parts = Vec::new();
        part(&mut parts, b"rs-events-1");
        fingerprint(&mut parts, record);
        part(&mut parts, self.context.identity.generation.as_bytes());
        match event_read::stream_optional(self.db, &record.facts.id)? {
            Some(stream) => {
                part(&mut parts, stream.generation.as_bytes());
                part(&mut parts, stream.complete_end.to_string().as_bytes());
                part(&mut parts, stream.partial_tail.to_string().as_bytes());
            }
            None => parts.push(b"-".to_vec()),
        }
        part(&mut parts, log.canonical().as_bytes());
        Ok(hex_digest(&parts))
    }

    /// Whether a cursor of another generation names this task and these kinds: its task
    /// digest is that of the id, and its filter digest that of the sorted kind list.
    pub fn foreign_cursor_binds(
        &self,
        task: &TaskId,
        kinds: &[String],
        task_digest: &str,
        filter_digest: &str,
    ) -> bool {
        let quoted: Vec<String> = kinds.iter().map(|kind| format!("\"{kind}\"")).collect();
        let filter = format!("[{}]", quoted.join(", "));
        hex_digest(&[task.as_str().as_bytes().to_vec()]) == task_digest
            && hex_digest(&[filter.into_bytes()]) == filter_digest
    }

    /// The stored prompt of a valid task; `None` when it has no prompt row.
    pub fn prompt(&self, id: &TaskId, max_bytes: usize) -> Result<Option<StoredPrompt>> {
        self.step()?;
        type Meta = (String, i64, i64, i64, i64, String, i64);
        let meta: Option<Meta> = self
            .db
            .query_row(statements::prompt_meta(), [id.as_str()], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .optional()?;
        let Some((form, inline, preview, title, name, blob_type, blob_len)) = meta else {
            return Ok(None);
        };
        let limit =
            i64::try_from(self.context.limits.max_bytes).map_err(|_| Error::LimitExceeded)?;
        let text_bytes = within(Some(inline + preview + title + name), limit)?;
        self.context.check_bounds(1, text_bytes)?;
        if !["blob", "null"].contains(&blob_type.as_str())
            || !["inline", "sidecar"].contains(&form.as_str())
        {
            return Err(Error::MalformedRow);
        }
        self.step()?;
        let (title, inline, preview, source): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self
            .db
            .query_row(statements::prompt_text(), [id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        let digest = source
            .as_deref()
            .and_then(|name| sidecar_digest(id.as_str(), name));
        let blob = match digest {
            Some(wanted) if form == "sidecar" && blob_type == "blob" => {
                self.sidecar(id, wanted, blob_len, max_bytes)?
            }
            _ => PromptBlob::Absent,
        };
        Ok(Some(StoredPrompt {
            title,
            inline,
            preview,
            bound: form == "sidecar" && digest.is_some(),
            blob,
        }))
    }

    fn sidecar(
        &self,
        id: &TaskId,
        wanted: &str,
        length: i64,
        max_bytes: usize,
    ) -> Result<PromptBlob> {
        self.step()?;
        let read = if length <= PROMPT_VERIFY_BYTES {
            length
        } else {
            (max_bytes + 4) as i64
        };
        let bytes: Vec<u8> = self.db.query_row(
            statements::prompt_blob(),
            rusqlite::params![id.as_str(), read],
            |r| r.get(0),
        )?;
        if length > PROMPT_VERIFY_BYTES {
            return Ok(PromptBlob::Unverified {
                prefix: bytes,
                total: u64::try_from(length).map_err(|_| Error::MalformedRow)?,
            });
        }
        if hex_digest(std::slice::from_ref(&bytes)) == wanted {
            Ok(PromptBlob::Verified(bytes))
        } else {
            Ok(PromptBlob::Mismatch)
        }
    }

    /// The manifest of a valid task, `None` when it has none.
    pub fn artifact_manifest(&self, id: &TaskId) -> Result<Option<ManifestHead>> {
        self.step()?;
        let row: Option<(i64, String, i64)> = self
            .db
            .query_row(statements::manifest(), [id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        row.map(|(valid, revision, count)| {
            Ok(ManifestHead {
                header_valid: valid == 1,
                revision,
                count: usize::try_from(count).map_err(|_| Error::MalformedRow)?,
            })
        })
        .transpose()
    }

    /// References in ordinal order from `offset`.
    pub fn artifact_items(
        &self,
        id: &TaskId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ArtifactItem>> {
        self.context.check_bounds(limit, 0)?;
        self.step()?;
        let (limit, offset) = (
            i64::try_from(limit).map_err(|_| Error::LimitExceeded)?,
            i64::try_from(offset).map_err(|_| Error::LimitExceeded)?,
        );
        let mut statement = self.db.prepare(statements::artifact_items())?;
        let mut rows = statement.query(rusqlite::params![id.as_str(), limit, offset])?;
        let mut found = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next()? {
            bytes += within(row.get(10)?, 4096)?
                + within(row.get(11)?, 4096)?
                + within(row.get(12)?, 1024)?
                + 96;
            self.context.check_bounds(found.len() + 1, bytes)?;
            found.push(ArtifactItem {
                artifact_id: row.get(0)?,
                path: row.get(1)?,
                kind: row.get(2)?,
                note: row.get(3)?,
                registered_at: row.get(4)?,
                method: row.get(5)?,
                registered: Observation {
                    device: unsigned(&row.get::<_, String>(6)?)?,
                    inode: unsigned(&row.get::<_, String>(7)?)?,
                    size: u64::try_from(row.get::<_, i64>(8)?).map_err(|_| Error::MalformedRow)?,
                    mtime_ns: row
                        .get::<_, String>(9)?
                        .parse()
                        .map_err(|_| Error::MalformedRow)?,
                },
            });
        }
        Ok(found)
    }

    /// The task a valid task continues: `None` when the task is missing or damaged.
    pub fn continuation_link(&self, id: &TaskId) -> Result<Option<Option<TaskId>>> {
        self.step()?;
        let link: Option<Option<String>> = self
            .db
            .query_row(statements::continuation(), [id.as_str()], |r| r.get(0))
            .optional()?;
        link.map(|text| text.map(task_id).transpose()).transpose()
    }
}

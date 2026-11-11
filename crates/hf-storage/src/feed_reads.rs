//! The reads behind recovery-activity, interaction-events and statistics-page. Every
//! variable-length value is measured in SQL before it is copied, and every query is bounded
//! by rows and by bytes.
use crate::{
    Error, Result,
    digest::Sha256Digest,
    event_read::capture_of,
    snapshot::TaskSnapshot,
    task_catalog::{task_id, within},
};
use hf_application::{
    ctl::task_reads::{ScanBatch, ScanRow},
    owner_runtime::DigestPort,
};
use hf_domain::{
    ctl::{
        feed::FeedSource,
        statistics::{GenerationEntry, StatFacts},
        task_rows::ChainRoot,
    },
    event_read::CaptureState,
    task_read::TaskId,
};
use rusqlite::{OptionalExtension, params};
use std::time::Instant;

/// Rows of one statistics batch.
const SCAN_ROWS: i64 = 200;
/// A task document beyond this is unreadable, whatever the query budget is.
const SOURCE_BYTES: i64 = 4 * 1024 * 1024;
const ROW_OVERHEAD: usize = 64;
const STATUS_BYTES: i64 = 256;

const FEED_COLUMNS: &str = "f.id,f.status,f.exit_code,substr(f.title,1,200),f.started_at_us,\
    f.finished_at_us,length(CAST(f.status AS BLOB)),length(CAST(f.id AS BLOB)),\
    coalesce(length(CAST(h.header AS BLOB)),0),coalesce(length(CAST(h.status AS BLOB)),0)";
const FEED_FROM: &str = "task_facts f JOIN task_headers h ON h.id=f.id";

/// Every text column the statistics scan copies, measured in bytes before any is read.
const STAT_BYTES: &str = "coalesce(length(CAST(f.id AS BLOB)),0)+coalesce(length(CAST(f.title AS BLOB)),0)\
    +coalesce(length(CAST(f.backend AS BLOB)),0)+coalesce(length(CAST(f.model AS BLOB)),0)\
    +coalesce(length(CAST(f.repo_path AS BLOB)),0)+coalesce(length(CAST(f.continuation_of AS BLOB)),0)\
    +coalesce(length(CAST(f.chain_root AS BLOB)),0)+coalesce(length(CAST(f.status AS BLOB)),0)\
    +coalesce(length(CAST(f.started_at AS BLOB)),0)+coalesce(length(CAST(f.finished_at AS BLOB)),0)";

fn feed_source(row: &rusqlite::Row<'_>) -> Result<FeedSource> {
    within(row.get(6)?, STATUS_BYTES)?;
    within(row.get(7)?, 64)?;
    let header = within(row.get(8)?, i64::MAX)?;
    let status = within(row.get(9)?, i64::MAX)?;
    Ok(FeedSource {
        id: row.get(0)?,
        status: row.get(1)?,
        exit_code: row.get(2)?,
        title: row.get(3)?,
        started_at_us: row.get(4)?,
        finished_at_us: row.get(5)?,
        metadata_bytes: (header + status) as u64,
    })
}

fn optional_id(text: Option<String>) -> Result<Option<TaskId>> {
    text.map(task_id).transpose()
}

impl TaskSnapshot<'_> {
    /// The capture the stream row stores; a row that does not decode is no capture.
    pub fn stream_capture(&self, id: &TaskId) -> Result<Option<CaptureState>> {
        self.step()?;
        let stored: Option<i64> = self
            .db
            .query_row(
                "SELECT capture FROM event_streams WHERE task=?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored.and_then(capture_of))
    }

    /// Valid tasks after `after`, in byte order of the id.
    pub fn feed_sources(&self, after: &str, limit: usize) -> Result<Vec<FeedSource>> {
        self.context.check_bounds(limit, 0)?;
        self.step()?;
        let sql = format!(
            "SELECT {FEED_COLUMNS} FROM {FEED_FROM} WHERE f.valid=1 AND f.id>?1 ORDER BY f.id LIMIT ?2"
        );
        let mut statement = self.db.prepare(&sql)?;
        let bound = i64::try_from(limit).map_err(|_| Error::LimitExceeded)?;
        let mut rows = statement.query(params![after, bound])?;
        let mut found = Vec::new();
        while let Some(row) = rows.next()? {
            found.push(feed_source(row)?);
        }
        Ok(found)
    }

    pub fn feed_source(&self, id: &str) -> Result<Option<FeedSource>> {
        self.step()?;
        let sql = format!("SELECT {FEED_COLUMNS} FROM {FEED_FROM} WHERE f.valid=1 AND f.id=?1");
        let mut statement = self.db.prepare(&sql)?;
        let mut rows = statement.query([id])?;
        rows.next()?.map(feed_source).transpose()
    }

    /// Task rows of any validity, counted up to `limit`.
    pub fn history_rows(&self, limit: usize) -> Result<usize> {
        self.step()?;
        let bound = i64::try_from(limit).map_err(|_| Error::LimitExceeded)?;
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM task_facts LIMIT ?1)",
            [bound],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| Error::MalformedRow)
    }

    /// Valid task rows, counted up to `limit`.
    pub fn valid_rows(&self, limit: usize) -> Result<usize> {
        self.step()?;
        let bound = i64::try_from(limit).map_err(|_| Error::LimitExceeded)?;
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM task_facts WHERE valid=1 LIMIT ?1)",
            [bound],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| Error::MalformedRow)
    }

    /// The next batch of valid tasks after `after`. A row over 4 MiB or over the query budget
    /// is returned as oversized without being read; a batch ends before the row that would
    /// pass the budget; once `deadline` passes the rows read so far are returned with `expired`.
    pub fn statistics_batch(&self, after: Option<&str>, deadline: Instant) -> Result<ScanBatch> {
        self.context.check_bounds(SCAN_ROWS as usize, 0)?;
        self.step()?;
        let sql = format!(
            "SELECT f.id,f.status,f.started_at,f.started_at_us,f.finished_at,f.finished_at_us,\
             f.exit_code,f.title,f.backend,f.model,f.repo_path,f.continuation_of,f.chain_root,\
             {STAT_BYTES} \
             FROM task_facts f \
             WHERE f.valid=1 AND (?1 IS NULL OR f.id>?1) ORDER BY f.id LIMIT ?2"
        );
        let mut statement = self.db.prepare(&sql)?;
        let mut rows = statement.query(params![after, SCAN_ROWS])?;
        let budget = self.context.limits.max_bytes;
        let mut found = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next()? {
            self.step()?;
            if Instant::now() >= deadline {
                return Ok(ScanBatch {
                    rows: found,
                    expired: true,
                });
            }
            let declared: i64 = row.get(13)?;
            let size = within(Some(declared), i64::MAX)? + ROW_OVERHEAD;
            // A row over the query budget is an unreadable source, as one over 4 MiB is.
            if declared > SOURCE_BYTES || size > budget {
                found.push(ScanRow::Oversized(row.get(0)?));
                continue;
            }
            if bytes + size > budget {
                break;
            }
            bytes += size;
            found.push(ScanRow::Facts(Box::new(stat_facts(row)?)));
        }
        Ok(ScanBatch {
            rows: found,
            expired: false,
        })
    }

    /// Hex SHA-256 over the store generation and the facts of the relevant tasks, written as
    /// compact JSON. An empty set is the constant digest of the application.
    pub fn statistics_generation(&self, entries: &[GenerationEntry]) -> Result<String> {
        self.step()?;
        let mut text = String::from("[\"rs-statistics-1\",");
        json_string(&mut text, &self.context.identity.generation);
        text.push_str(",[");
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            entry_json(&mut text, entry);
        }
        text.push_str("]]");
        let digest = Sha256Digest.digest_parts(&[text.as_bytes()]);
        Ok(digest.as_str().to_owned())
    }
}

fn stat_facts(row: &rusqlite::Row<'_>) -> Result<StatFacts> {
    Ok(StatFacts {
        id: task_id(row.get(0)?)?,
        status: row.get(1)?,
        started_at: row.get(2)?,
        started_at_us: row.get(3)?,
        finished_at: row.get(4)?,
        finished_at_us: row.get(5)?,
        exit_code: row.get(6)?,
        title: row.get(7)?,
        backend: row.get(8)?,
        model: row.get(9)?,
        repo_path: row.get(10)?,
        continuation_of: optional_id(row.get(11)?)?,
        chain_root: ChainRoot::qualify(row.get(12)?),
    })
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_optional(out: &mut String, text: Option<&str>) {
    match text {
        Some(text) => json_string(out, text),
        None => out.push_str("null"),
    }
}

fn json_number(out: &mut String, number: Option<i64>) {
    match number {
        Some(number) => out.push_str(&number.to_string()),
        None => out.push_str("null"),
    }
}

/// `[key, form, facts...]`: all projected facts, or only the identity of an ancestor that
/// has not finished.
fn entry_json(out: &mut String, entry: &GenerationEntry) {
    let facts = &entry.facts;
    out.push('[');
    json_string(out, &format!("task:{}", facts.id.as_str()));
    out.push(',');
    let link = facts.continuation_of.as_ref().map(TaskId::as_str);
    let root = facts.chain_root.raw();
    if entry.identity_only {
        out.push_str("\"nonterminal_ancestry\",");
        json_optional(out, link);
        out.push(',');
        json_optional(out, root);
        out.push(']');
        return;
    }
    out.push_str("\"facts\",");
    json_string(out, &facts.status);
    out.push(',');
    json_number(out, Some(facts.started_at_us));
    out.push(',');
    json_number(out, facts.finished_at_us);
    out.push(',');
    json_number(out, facts.exit_code);
    out.push(',');
    json_optional(out, facts.title.as_deref());
    out.push(',');
    json_string(out, &facts.backend);
    out.push(',');
    json_optional(out, facts.model.as_deref());
    out.push(',');
    json_string(out, &facts.repo_path);
    out.push(',');
    json_optional(out, link);
    out.push(',');
    json_optional(out, root);
    out.push(']');
}

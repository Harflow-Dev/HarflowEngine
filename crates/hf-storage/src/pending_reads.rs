//! The pending launch reads of the ctl: the launch namespace, its rows in receipt filename
//! order, and the check of every ordinary record that the direct-only caller gate needs.
//! All of it runs inside the one read transaction of the snapshot.
use crate::{
    Error, Result,
    digest::{Sha256Digest, Sha256Stream},
    snapshot::TaskSnapshot,
};
use hf_application::{
    ctl::task_reads::{Namespace, PendingBinding, PendingBudget, PendingLimits},
    owner_runtime::DigestPort,
};
use hf_domain::{
    ctl::pending::{LAUNCH_MAX_BYTES, LaunchFacts, RECORD_SUFFIX, SUFFIX},
    task_read::TaskId,
};
use rusqlite::{OptionalExtension, Row, params, types::ValueRef};

/// Rows between two looks at the deadline.
const STEP_ROWS: usize = 256;

/// The raw size of a launch row's text columns, from their lengths alone.
const RAW_SIZE: &str = "SELECT length(CAST(name AS BLOB))+coalesce(length(CAST(generation AS BLOB)),0)\
    +coalesce(length(CAST(state AS BLOB)),0)+coalesce(length(CAST(title AS BLOB)),0)\
    +coalesce(length(CAST(backend AS BLOB)),0)+coalesce(length(CAST(repo_path AS BLOB)),0)\
    +coalesce(length(CAST(created_at AS BLOB)),0)+coalesce(length(CAST(not_started_reason AS BLOB)),0)\
    +coalesce(length(CAST(spawned_by AS BLOB)),0)+coalesce(length(CAST(root_task_id AS BLOB)),0)\
    +coalesce(length(CAST(parent_task_id AS BLOB)),0)+coalesce(length(CAST(chain_root_task_id AS BLOB)),0)\
    +coalesce(length(CAST(resource_names AS BLOB)),0)+coalesce(length(CAST(refusal_code AS BLOB)),0)\
    +coalesce(length(CAST(refusal_check AS BLOB)),0) FROM launch_attempts \
    WHERE name=?1 AND task_id IS NOT NULL";

const ROW: &str = "SELECT name,generation,state,title,backend,repo_path,created_at,not_started_reason,\
    spawned_by,root_task_id,parent_task_id,chain_root_task_id,resource_names,has_resources,\
    has_worktree,refusal_code,refusal_check,\
    EXISTS(SELECT 1 FROM task_headers h WHERE h.id=launch_attempts.task_id) FROM launch_attempts \
    WHERE task_id IS NOT NULL";

/// A column as text: `None` for NULL, and whether the value is text the wire can carry.
fn text(row: &Row<'_>, index: usize) -> Result<(Option<String>, bool)> {
    Ok(match row.get_ref(index)? {
        ValueRef::Null => (None, true),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => match std::str::from_utf8(bytes) {
            Ok(value) => (Some(value.to_owned()), true),
            Err(_) => (None, false),
        },
        _ => (None, false),
    })
}

fn flag(row: &Row<'_>, index: usize) -> Result<bool> {
    Ok(row.get::<_, Option<i64>>(index)? == Some(1))
}

impl TaskSnapshot<'_> {
    /// What a pending cursor is bound to: the selected root and the store.
    pub fn pending_binding(&self, root: &str) -> Result<PendingBinding> {
        let digest = |text: &str| {
            Sha256Digest
                .digest_parts(&[text.as_bytes()])
                .as_str()
                .to_owned()
        };
        let generation = &self.context.identity.generation;
        let source = if generation.len() == 32 && generation.bytes().all(|b| b.is_ascii_hexdigit())
        {
            generation.clone()
        } else {
            digest(generation)[..32].to_owned()
        };
        let mut identity = [0u64; 4];
        for (index, part) in identity.iter_mut().enumerate() {
            *part = u64::from_str_radix(&source[index * 8..index * 8 + 8], 16)
                .map_err(|_| Error::MalformedRow)?;
        }
        Ok(PendingBinding {
            root: digest(root),
            identity,
        })
    }

    /// One pass over every ordinary record of the store in this snapshot. `false` when the
    /// history is incomplete, a record cannot be qualified, a record has an unusable
    /// identity, the records and receipts exceed the namespace bounds, or the read budget or
    /// the deadline runs out: the caller can then not be established.
    pub fn candidates_checked(
        &self,
        budget: &PendingBudget,
        limits: &PendingLimits,
    ) -> Result<bool> {
        match self.check_candidates(budget, limits) {
            Err(Error::DeadlineExceeded | Error::Busy) => Ok(false),
            other => other,
        }
    }

    fn check_candidates(&self, budget: &PendingBudget, limits: &PendingLimits) -> Result<bool> {
        if self.damaged()? || !self.directory_within(limits)? {
            return Ok(false);
        }
        self.step()?;
        let mut statement = self.db.prepare(
            "SELECT h.id,f.valid,coalesce(length(CAST(h.header AS BLOB)),0)\
             +coalesce(length(CAST(h.status AS BLOB)),0) \
             FROM task_headers h LEFT JOIN task_facts f ON f.id=h.id",
        )?;
        let mut rows = statement.query([])?;
        let mut seen = 0usize;
        while let Some(row) = rows.next()? {
            seen += 1;
            if seen.is_multiple_of(STEP_ROWS) {
                self.step()?;
            }
            let (id, valid) = (text(row, 0)?, row.get::<_, Option<i64>>(1)?);
            let identity = id.0.is_some_and(|id| TaskId::new(id).is_ok());
            let charge = u64::try_from(row.get::<_, i64>(2)?).unwrap_or(u64::MAX);
            if !identity || valid != Some(1) || !budget.charge(charge) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether the records and receipts together stay within the entry and name-byte bounds
    /// the reference applies to the one directory that holds them.
    fn directory_within(&self, limits: &PendingLimits) -> Result<bool> {
        self.step()?;
        let (entries, bytes): (i64, i64) = self.db.query_row(
            "SELECT (SELECT count(*) FROM task_headers)+(SELECT count(*) FROM launch_attempts),\
             (SELECT coalesce(sum(length(CAST(id AS BLOB))+?1),0) FROM task_headers)\
             +(SELECT coalesce(sum(length(CAST(name AS BLOB))+?2),0) FROM launch_attempts)",
            params![RECORD_SUFFIX.len() as i64, SUFFIX.len() as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let within = |value: i64, max: usize| usize::try_from(value).is_ok_and(|v| v <= max);
        Ok(within(entries, limits.entries) && within(bytes, limits.name_bytes))
    }

    /// The namespace of receipt names; `None` beyond the entry or name-byte bound.
    pub fn pending_namespace(&self, limits: &PendingLimits) -> Result<Option<Namespace>> {
        self.step()?;
        let (count, bytes): (i64, i64) = self.db.query_row(
            "SELECT count(*),coalesce(sum(length(CAST(name AS BLOB))),0) FROM launch_attempts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let entries = usize::try_from(count).map_err(|_| Error::MalformedRow)?;
        let total = usize::try_from(bytes)
            .map_err(|_| Error::MalformedRow)?
            .saturating_add(entries.saturating_mul(SUFFIX.len()));
        if entries > limits.entries || total > limits.name_bytes {
            return Ok(None);
        }
        let mut digest = Sha256Stream::with_limit(total.saturating_add(entries))
            .map_err(|_| Error::LimitExceeded)?;
        let mut statement = self.db.prepare(
            "SELECT name,task_id IS NULL FROM launch_attempts ORDER BY name||'.launch.json'",
        )?;
        let mut rows = statement.query([])?;
        let (mut invalid, mut seen) = (0u64, 0usize);
        while let Some(row) = rows.next()? {
            seen += 1;
            if seen.is_multiple_of(STEP_ROWS) {
                self.step()?;
            }
            let ValueRef::Text(name) = row.get_ref(0)? else {
                return Err(Error::MalformedRow);
            };
            for part in [name, SUFFIX.as_bytes(), &[0u8]] {
                digest.update(part).map_err(|_| Error::LimitExceeded)?;
            }
            invalid += u64::from(row.get::<_, i64>(1)? == 1);
        }
        let revision = digest.finish().map_err(|_| Error::LimitExceeded)?;
        Ok(Some(Namespace {
            revision: revision.as_str().to_owned(),
            invalid,
        }))
    }

    /// Whether `id` is a receipt name that is a task identity.
    pub fn launch_name_exists(&self, id: &str) -> Result<bool> {
        self.step()?;
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM launch_attempts WHERE name=?1 AND task_id IS NOT NULL)",
            [id],
            |row| row.get(0),
        )?)
    }

    /// The id of the first launch row named by a task identity whose filename sorts after
    /// that of `after`. Only the name is read.
    pub fn next_launch_name(&self, after: Option<&str>) -> Result<Option<String>> {
        self.step()?;
        let filter = after.map(|id| format!("{id}{SUFFIX}"));
        Ok(self
            .db
            .query_row(
                "SELECT name FROM launch_attempts WHERE task_id IS NOT NULL \
                 AND (?1 IS NULL OR name||'.launch.json'>?1) ORDER BY name||'.launch.json' LIMIT 1",
                [filter],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// The launch row of `id`, a bare record when only a record has the id, or nothing.
    ///
    /// A row whose raw text columns exceed `LAUNCH_MAX_BYTES` is refused as `oversize`
    /// before any of it is decoded.
    pub fn pending_row(&self, id: &TaskId) -> Result<Option<LaunchFacts>> {
        self.step()?;
        let size: Option<i64> = self
            .db
            .query_row(RAW_SIZE, [id.as_str()], |row| row.get(0))
            .optional()?;
        if size.is_some_and(|size| u64::try_from(size).map_or(true, |s| s > LAUNCH_MAX_BYTES)) {
            return Ok(Some(LaunchFacts {
                id: id.as_str().to_owned(),
                refusal: Some(("oversize".into(), "oversize".into())),
                ..LaunchFacts::default()
            }));
        }
        self.step()?;
        let mut statement = self.db.prepare(&format!("{ROW} AND name=?1"))?;
        let mut rows = statement.query([id.as_str()])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(self.facts(row)?));
        }
        self.step()?;
        let record: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_headers WHERE id=?1)",
            [id.as_str()],
            |row| row.get(0),
        )?;
        Ok(record.then(|| LaunchFacts {
            id: id.as_str().to_owned(),
            record: true,
            ..LaunchFacts::default()
        }))
    }

    /// The raw stored size of the ordinary record `id`, read from column lengths.
    pub fn record_size(&self, id: &TaskId) -> Result<Option<u64>> {
        self.step()?;
        let size: Option<i64> = self
            .db
            .query_row(
                "SELECT coalesce(length(CAST(header AS BLOB)),0)+coalesce(length(CAST(status AS BLOB)),0) \
                 FROM task_headers WHERE id=?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(size.map(|size| u64::try_from(size).unwrap_or(u64::MAX)))
    }

    fn facts(&self, row: &Row<'_>) -> Result<LaunchFacts> {
        let plain = |index| text(row, index).map(|pair| pair.0);
        let mut text_valid = true;
        let mut display = |index| -> Result<Option<String>> {
            let (value, ok) = text(row, index)?;
            text_valid &= ok;
            Ok(value)
        };
        let (title, backend, repo_path) = (display(3)?, display(4)?, display(5)?);
        let (created_at, not_started_reason) = (display(6)?, display(7)?);
        let names = match plain(12)? {
            Some(json) => self.names(&json)?,
            None => None,
        };
        Ok(LaunchFacts {
            id: plain(0)?.ok_or(Error::MalformedRow)?,
            record: flag(row, 17)?,
            state: plain(2)?,
            refusal: plain(15)?.zip(plain(16)?),
            generation: plain(1)?,
            title,
            backend,
            repo_path,
            created_at,
            not_started_reason,
            spawned_by: plain(8)?,
            root_task_id: plain(9)?,
            parent_task_id: plain(10)?,
            chain_root_task_id: plain(11)?,
            resource_names: names,
            has_resources: flag(row, 13)?,
            has_worktree: flag(row, 14)?,
            text_valid,
        })
    }

    /// The elements of a stored JSON list when every one is text.
    fn names(&self, json: &str) -> Result<Option<Vec<String>>> {
        let mut statement = self.db.prepare("SELECT type,value FROM json_each(?1)")?;
        let mut rows = statement.query([json])?;
        let mut names = Vec::new();
        while let Some(row) = rows.next()? {
            if row.get::<_, String>(0)? != "text" {
                return Ok(None);
            }
            names.push(row.get::<_, String>(1)?);
        }
        Ok(Some(names))
    }
}

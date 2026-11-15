//! The owner-evidence rows of an open store connection: every recorded process identity
//! (terminal or not) and every record that cannot establish its own state.
use crate::{Error, Result, ownership_codec, scan};
use hf_application::ctl::state_recover::evidence::{Item, Source, Subject};
use hf_domain::{ctl::caller::is_candidate, ownership::Phase, process::ProcessIdentity};
use rusqlite::{Connection, OptionalExtension};
use std::{collections::HashSet, time::Instant};

/// `pid`, `start_seconds`, `start_micros`, `boot_uuid` of one `task_processes` row.
type ProcessRow = (i64, Option<i64>, Option<i64>, Option<String>);

/// What the queries returned, before it is decoded.
struct Raw {
    ownership: Vec<(i64, Option<String>, Option<String>)>,
    processes: Vec<ProcessRow>,
    damaged: bool,
    unobserved: bool,
    launch_without_identity: bool,
}

/// Reads the items in one read transaction, within `deadline`. Any value that does not
/// decode, and any SQLite failure or interrupt, is an error (the basis is then unreadable).
pub(crate) fn read(connection: &Connection, deadline: Instant) -> Result<Vec<Item>> {
    let raw = scan::bounded(connection, deadline, |db| {
        let tx = db.unchecked_transaction()?;
        let raw = Raw {
            ownership: collect(
                &tx,
                "SELECT phase,owner_process,child FROM ownership_records",
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?,
            processes: collect(
                &tx,
                "SELECT pid,start_seconds,start_micros,boot_uuid FROM task_processes",
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?,
            damaged: exists(&tx, "SELECT 1 FROM task_facts WHERE valid=0 LIMIT 1")?,
            unobserved: unobserved(&tx)?,
            launch_without_identity: exists(
                &tx,
                "SELECT 1 FROM launch_attempts l WHERE l.task_id IS NOT NULL \
                 AND l.state IN('claimed','granted','spawned') \
                 AND NOT EXISTS(SELECT 1 FROM task_processes p WHERE p.task=l.task_id) LIMIT 1",
            )?,
        };
        tx.finish()?;
        Ok(raw)
    })?;
    decode(raw)
}

fn collect<T>(
    db: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    db.prepare(sql)?.query_map([], map)?.collect()
}

fn exists(db: &Connection, sql: &str) -> rusqlite::Result<bool> {
    Ok(db.query_row(sql, [], |_| Ok(())).optional()?.is_some())
}

/// A valid task with no process row that is not observed terminal: nothing says whether
/// its agent exited.
fn unobserved(db: &Connection) -> rusqlite::Result<bool> {
    let mut statement = db.prepare(
        "SELECT f.status,f.exit_code FROM task_facts f WHERE f.valid=1 \
         AND NOT EXISTS(SELECT 1 FROM task_processes p WHERE p.task=f.id)",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let status: Option<String> = row.get(0)?;
        let code: Option<i64> = row.get(1)?;
        if status.is_none_or(|status| is_candidate(&status, code)) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn decode(raw: Raw) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut identity = |source, value: ProcessIdentity| {
        if seen.insert(ownership_codec::process(&value)) {
            items.push(Item {
                source,
                subject: Subject::Identity(value),
            });
        }
    };
    let mut ambiguous = Vec::new();
    for (phase, owner, child) in &raw.ownership {
        if let Some(text) = owner {
            identity(
                Source::OwnershipOwner,
                ownership_codec::decode_process(text)?,
            );
        }
        if let Some(text) = child {
            identity(
                Source::OwnershipChild,
                ownership_codec::decode_process(text)?,
            );
        }
        if ownership_codec::phase(*phase)? == Phase::Claimed && child.is_none() {
            ambiguous.push(Source::OwnershipChild);
        }
    }
    let mut pids = Vec::new();
    for (pid, seconds, micros, boot) in &raw.processes {
        let pid = u32::try_from(*pid).map_err(|_| Error::Incompatible)?;
        match (seconds, micros, boot) {
            (Some(seconds), Some(micros), Some(boot)) => {
                let value = ProcessIdentity::new(
                    pid,
                    u64::try_from(*seconds).map_err(|_| Error::Incompatible)?,
                    u64::try_from(*micros).map_err(|_| Error::Incompatible)?,
                    boot.clone(),
                )
                .map_err(|_| Error::Incompatible)?;
                identity(Source::TaskProcess, value);
            }
            (None, None, None) => pids.push(pid),
            _ => return Err(Error::Incompatible),
        }
    }
    items.extend(pids.into_iter().map(|pid| Item {
        source: Source::TaskProcess,
        subject: Subject::Pid(pid),
    }));
    if raw.damaged || raw.unobserved {
        ambiguous.push(Source::TaskRecord);
    }
    if raw.launch_without_identity {
        ambiguous.push(Source::LaunchAttempt);
    }
    items.extend(ambiguous.into_iter().map(|source| Item {
        source,
        subject: Subject::Undecidable,
    }));
    Ok(items)
}

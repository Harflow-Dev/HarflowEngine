//! Operations a measured child or warm loop performs: the existing open path and
//! a candidate read-only path. The candidate is measured as open plus
//! representative SQL; production queries are bound to the read-write handle.
use super::{common::lease::lease_port, data};
use hf_application::lease::{LeaseGuard, LeaseMode};
use hf_domain::{
    event_read::{EventFilter, EventQuery},
    task_read::{AuthorizedScope, PageFilter, TaskId, TaskQuery},
};
use hf_storage::{APPLICATION_ID, SCHEMA_VERSION, Store};
use rusqlite::{Connection, OpenFlags, config::DbConfig};
use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

pub struct Timing {
    pub open_us: u64,
    pub query_us: u64,
    pub err: String,
}

pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

pub fn micros(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_micros()).unwrap_or(u64::MAX)
}

pub fn error_name(error: &impl std::fmt::Debug) -> String {
    let text = format!("{error:?}");
    let end = text.find(['(', ' ', '{']).unwrap_or(text.len());
    text[..end]
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn old_query(store: &Store, kind: u64, task: &TaskId) -> Result<usize, String> {
    let scope = AuthorizedScope::Global;
    match kind {
        0 => store
            .task_read(&scope, &TaskQuery::StoredStatus(task.clone()), deadline())
            .map(|r| r.rows.len()),
        1 => {
            let page = TaskQuery::page(50, PageFilter::new(false, None).unwrap(), None).unwrap();
            store
                .task_read(&scope, &page, deadline())
                .map(|r| r.rows.len())
        }
        _ => {
            let query = EventQuery {
                task: task.clone(),
                limit: 50,
                filter: EventFilter::Default,
                from_oldest: false,
                cursor: None,
            };
            store.event_read(&query, deadline()).map(|r| r.events.len())
        }
    }
    .map_err(|e| error_name(&e))
}

pub fn old_open(root: &Path) -> Result<Store, String> {
    Store::open(root, data::limits(), deadline(), lease_port()).map_err(|e| error_name(&e))
}

pub fn old_once(root: &Path, kind: u64, task: &TaskId) -> Timing {
    let started = Instant::now();
    let store = old_open(root);
    let open_us = micros(started);
    let started = Instant::now();
    let result = store.and_then(|s| old_query(&s, kind, task));
    Timing {
        open_us,
        query_us: micros(started),
        err: result.err().unwrap_or_default(),
    }
}

pub struct Candidate {
    pub conn: Connection,
    _lease: Box<dyn LeaseGuard>,
}

pub fn marker(root: &Path) -> Result<String, String> {
    let mut text = String::new();
    fs::File::open(root.join("store.identity"))
        .and_then(|f| f.take(129).read_to_string(&mut text))
        .map_err(|_| "MarkerIo".to_owned())?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 3 || lines[0] != "harflow-storage-2" || lines[1] != SCHEMA_VERSION.to_string()
    {
        return Err("MarkerIncompatible".into());
    }
    Ok(lines[2].to_owned())
}

pub fn header(path: &Path) -> Result<(), String> {
    let mut h = [0u8; 100];
    let mut file = fs::File::open(path).map_err(|_| "HeaderIo".to_owned())?;
    file.read_exact(&mut h)
        .map_err(|_| "HeaderShort".to_owned())?;
    let word = |at: usize| u32::from_be_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
    if &h[..16] != b"SQLite format 3\0" || word(68) != APPLICATION_ID || word(60) != SCHEMA_VERSION
    {
        return Err("HeaderIncompatible".into());
    }
    Ok(())
}

pub fn ro_open(root: &Path) -> Result<Candidate, String> {
    let lease = lease_port()
        .acquire(
            &root.join("store.lock"),
            LeaseMode::Shared,
            false,
            deadline(),
        )
        .map_err(|e| error_name(&e))?;
    let expected = marker(root)?;
    let db = root.join("store.sqlite3");
    header(&db)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(&db, flags).map_err(|e| error_name(&e))?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .map_err(|e| error_name(&e))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| error_name(&e))?;
    let seen: String = conn
        .query_row(
            "SELECT generation FROM store_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| error_name(&e))?;
    if seen != expected {
        return Err("IdentityMismatch".into());
    }
    Ok(Candidate {
        conn,
        _lease: lease,
    })
}

pub fn ro_query(candidate: &Candidate, kind: u64, task: &TaskId) -> Result<usize, String> {
    let sql = match kind {
        0 => {
            "SELECT stamp,session,active,recorded_status,header,status FROM task_headers WHERE id=?1"
        }
        1 => {
            "SELECT id,stamp,session,active,header FROM task_headers ORDER BY stamp DESC,id DESC LIMIT 50+?1*0"
        }
        _ => {
            "SELECT start,end,class,seq,kind,body FROM event_records WHERE task=?1 ORDER BY start DESC LIMIT 50"
        }
    };
    let mut statement = candidate.conn.prepare(sql).map_err(|e| error_name(&e))?;
    let columns = statement.column_count();
    let mut rows = statement
        .query([task.as_str()])
        .map_err(|e| error_name(&e))?;
    let (mut count, mut bytes) = (0, 0usize);
    while let Some(row) = rows.next().map_err(|e| error_name(&e))? {
        count += 1;
        // Every column is decoded, as the production queries do.
        for column in 0..columns {
            bytes += match row.get_ref(column).map_err(|e| error_name(&e))? {
                rusqlite::types::ValueRef::Blob(b) => b.len(),
                rusqlite::types::ValueRef::Text(t) => t.len(),
                _ => 8,
            };
        }
    }
    std::hint::black_box(bytes);
    Ok(count)
}

pub fn old_events(
    store: &Store,
    task: &TaskId,
) -> Result<Vec<hf_application::event_read::EventRecord>, String> {
    let query = EventQuery {
        task: task.clone(),
        limit: 50,
        filter: EventFilter::Default,
        from_oldest: false,
        cursor: None,
    };
    store
        .event_read(&query, deadline())
        .map(|r| r.events)
        .map_err(|e| error_name(&e))
}

pub fn ro_once(root: &Path, kind: u64, task: &TaskId) -> Timing {
    let started = Instant::now();
    let candidate = ro_open(root);
    let open_us = micros(started);
    let started = Instant::now();
    let result = candidate.and_then(|c| ro_query(&c, kind, task));
    Timing {
        open_us,
        query_us: micros(started),
        err: result.err().unwrap_or_default(),
    }
}

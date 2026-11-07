//! The SQL of the ctl task reads, built in one place. The snapshot runs exactly these
//! statements, and storage tests inspect their query plans through the same functions.
//! The ctl host may name only the session, so none of this is reachable from ctl code.
use hf_domain::ctl::{caller::TERMINAL_STATUSES, task_rows::BatchQuery};
use rusqlite::types::Value;

const COLUMNS: &str = "f.id,f.backend,f.repo_path,f.session_id,f.status,f.exit_code,f.started_at,\
    f.started_at_us,f.finished_at,f.finished_at_us,f.output_drained,f.title,f.model,\
    f.continuation_of,f.chain_root,h.header,h.status,p.pid,p.start_seconds,p.start_micros,p.boot_uuid,\
    length(CAST(h.header AS BLOB)),coalesce(length(CAST(h.status AS BLOB)),0),\
    coalesce(length(CAST(f.backend AS BLOB)),0)+coalesce(length(CAST(f.repo_path AS BLOB)),0)\
    +coalesce(length(CAST(f.session_id AS BLOB)),0)+coalesce(length(CAST(f.status AS BLOB)),0)\
    +coalesce(length(CAST(f.started_at AS BLOB)),0)+coalesce(length(CAST(f.finished_at AS BLOB)),0)\
    +coalesce(length(CAST(f.title AS BLOB)),0)+coalesce(length(CAST(f.model AS BLOB)),0)\
    +coalesce(length(CAST(f.continuation_of AS BLOB)),0)+coalesce(length(CAST(f.chain_root AS BLOB)),0)\
    +coalesce(length(CAST(p.boot_uuid AS BLOB)),0)";
const FROM: &str = "task_facts f JOIN task_headers h ON h.id=f.id \
    LEFT JOIN task_processes p ON p.task=f.id";
/// The expression the partial index `facts_unobserved` is declared with.
pub const UNOBSERVED: &str = "f.valid=1 AND (f.status='running' OR f.exit_code IS NULL)";

/// One record by id.
pub fn record() -> String {
    format!("SELECT {COLUMNS} FROM {FROM} WHERE f.valid=1 AND f.id=?1")
}

/// A keyset batch, newest first. Order, filters and `after` all use `started_at_us`, the
/// column the partial and session indexes cover.
pub fn batch(query: &BatchQuery) -> (String, Vec<Value>) {
    let mut predicate = String::from("f.valid=1");
    let mut params: Vec<Value> = Vec::new();
    if let Some(session) = &query.session {
        predicate.push_str(" AND f.session_id=?");
        params.push(Value::Text(session.clone()));
    }
    if let Some(predecessor) = &query.predecessor {
        predicate.push_str(" AND f.continuation_of=?");
        params.push(Value::Text(predecessor.as_str().to_owned()));
    }
    if let Some(since) = query.since_us {
        predicate.push_str(" AND f.started_at_us>=?");
        params.push(Value::Integer(since));
    }
    if query.unobserved_only {
        predicate.push_str(" AND ");
        predicate.push_str(UNOBSERVED);
    }
    if let Some((stamp, id)) = &query.after {
        predicate.push_str(" AND (f.started_at_us,f.id)<(?,?)");
        params.push(Value::Integer(*stamp));
        params.push(Value::Text(id.clone()));
    }
    params.push(Value::Integer(
        i64::try_from(query.max_rows).unwrap_or(i64::MAX),
    ));
    let sql = format!(
        "SELECT {COLUMNS} FROM {FROM} WHERE {predicate} \
         ORDER BY f.started_at_us DESC,f.id DESC LIMIT ?"
    );
    (sql, params)
}

pub fn unobserved_count() -> String {
    format!("SELECT count(*) FROM task_facts f WHERE {UNOBSERVED}")
}

pub fn unobserved_rows(has_after: bool) -> String {
    let after = if has_after {
        " AND (f.started_at_us,f.id)<(?,?)"
    } else {
        ""
    };
    format!(
        "SELECT f.id,f.started_at_us,f.session_id,f.status,f.exit_code,p.pid,p.start_seconds,\
         p.start_micros,p.boot_uuid,coalesce(length(CAST(f.session_id AS BLOB)),0),\
         coalesce(length(CAST(f.status AS BLOB)),0),coalesce(length(CAST(p.boot_uuid AS BLOB)),0) \
         FROM task_facts f LEFT JOIN task_processes p ON p.task=f.id \
         WHERE {UNOBSERVED}{after} ORDER BY f.started_at_us DESC,f.id DESC LIMIT ?"
    )
}

/// Related unfinished tasks: a process row counts even when its task row is missing, and
/// such a row can never be decided (the caller drops its start identity).
pub fn related_callers(ancestors: usize) -> String {
    let terminal = TERMINAL_STATUSES.map(|name| format!("'{name}'")).join(",");
    let pids = if ancestors == 0 {
        String::new()
    } else {
        format!(" OR p.pid IN ({})", vec!["?"; ancestors].join(","))
    };
    format!(
        "SELECT p.task,coalesce(f.status,''),f.exit_code,p.pid,p.start_seconds,p.start_micros,\
         p.boot_uuid,coalesce(length(CAST(f.status AS BLOB)),0),\
         coalesce(length(CAST(p.boot_uuid AS BLOB)),0),f.id IS NULL \
         FROM task_processes p LEFT JOIN task_facts f ON f.id=p.task \
         WHERE (f.id IS NULL OR (f.valid=1 AND NOT (f.status IN ({terminal}) AND f.exit_code IS NOT NULL))) \
         AND (p.pgid=?{pids}) AND p.task>? ORDER BY p.task LIMIT ?"
    )
}

pub fn summary() -> &'static str {
    "SELECT id,substr(title,1,200),status,started_at,length(CAST(status AS BLOB)),\
     length(CAST(started_at AS BLOB)) FROM task_facts WHERE id=?1 AND valid=1"
}

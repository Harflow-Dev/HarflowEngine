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

/// One record by id with the stop row of its chain root, joined by the table's primary key
/// in the same statement: columns 24 and 25 are the row's kind and its attribution, the
/// attribution only when it is 1 to 64 bytes (nothing longer is ever copied). The join key
/// is the stored root, or the task itself when none is stored.
pub fn record_with_stop() -> String {
    format!(
        "SELECT {COLUMNS},c.kind,CASE WHEN length(CAST(c.cancelled_task_id AS BLOB)) BETWEEN 1 AND 64 \
         THEN c.cancelled_task_id END FROM {FROM} \
         LEFT JOIN chain_stops c ON c.root=coalesce(f.chain_root,f.id) WHERE f.valid=1 AND f.id=?1"
    )
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

/// The prompt row of a valid task, with the lengths of every value measured in SQL so
/// nothing is copied before it is checked.
pub fn prompt_meta() -> &'static str {
    "SELECT p.form,coalesce(length(CAST(p.inline_text AS BLOB)),0),\
     coalesce(length(CAST(p.preview AS BLOB)),0),coalesce(length(CAST(f.title AS BLOB)),0),\
     coalesce(length(CAST(p.source_name AS BLOB)),0),typeof(p.blob),coalesce(length(p.blob),0) \
     FROM task_prompts p JOIN task_facts f ON f.id=p.task WHERE p.task=?1 AND f.valid=1"
}

pub fn prompt_text() -> &'static str {
    "SELECT f.title,p.inline_text,p.preview,p.source_name FROM task_prompts p \
     JOIN task_facts f ON f.id=p.task WHERE p.task=?1"
}

pub fn prompt_blob() -> &'static str {
    "SELECT substr(blob,1,?2) FROM task_prompts WHERE task=?1"
}

/// The manifest of a valid task and its item count, read up to 101.
pub fn manifest() -> &'static str {
    "SELECT m.header_valid,m.revision,(SELECT count(*) FROM \
     (SELECT 1 FROM artifact_items i WHERE i.task=m.task LIMIT 101)) \
     FROM artifact_manifests m JOIN task_facts f ON f.id=m.task WHERE m.task=?1 AND f.valid=1"
}

/// One page of references, by ordinal. The text columns are measured before they are read.
pub fn artifact_items() -> &'static str {
    "SELECT i.artifact_id,i.path,i.kind,i.note,i.registered_at,i.provenance_method,\
     i.device,i.inode,i.size,i.mtime_ns,length(CAST(i.path AS BLOB)),length(CAST(i.note AS BLOB)),\
     length(CAST(i.artifact_id AS BLOB))+length(CAST(i.kind AS BLOB))\
     +length(CAST(i.registered_at AS BLOB))+length(CAST(i.provenance_method AS BLOB))\
     ,\
     length(CAST(coalesce(i.tool_use_id,'') AS BLOB)),\
     length(CAST(coalesce(i.plan_directory,'') AS BLOB)),\
     i.tool_use_id,i.plan_directory,i.plan_device,i.plan_inode \
     FROM artifact_items i JOIN task_facts f ON f.id=i.task AND f.valid=1 \
     WHERE i.task=?1 ORDER BY i.ordinal LIMIT ?2 OFFSET ?3"
}

/// The link a valid task continues from; no row for a missing or damaged task.
pub fn continuation() -> &'static str {
    "SELECT continuation_of FROM task_facts WHERE id=?1 AND valid=1"
}

/// Stored bodies of valid event records inside `[?2, ?3]` of the complete log, newest
/// first, with the real length of each. A body over the stored limit is not read.
pub fn recent_bodies() -> &'static str {
    "SELECT CASE WHEN typeof(body)='blob' THEN length(body) END,\
     CASE WHEN typeof(body)='blob' AND length(body)<=16384 THEN body END \
     FROM event_records INDEXED BY event_starts \
     WHERE task=?1 AND start>=?2 AND end<=?3 AND class=0 ORDER BY start DESC"
}

/// The stored blobs' lengths, measured before any JSON is parsed.
pub fn blob_lengths() -> &'static str {
    "SELECT length(CAST(h.header AS BLOB)),coalesce(length(CAST(h.status AS BLOB)),0) \
     FROM task_headers h WHERE h.id=?1"
}

/// Python truthiness of `workflow_builder` in one blob: true, a non-zero number, a
/// non-empty string, a non-empty list or object. A blob that is not JSON says nothing.
fn builder_of(column: &str) -> String {
    let text = format!("CAST({column} AS TEXT)");
    let path = "'$.workflow_builder'";
    format!(
        "CASE WHEN {column} IS NOT NULL AND json_valid({text}) THEN \
         CASE json_type({text},{path}) \
         WHEN 'true' THEN 1 \
         WHEN 'integer' THEN json_extract({text},{path})<>0 \
         WHEN 'real' THEN json_extract({text},{path})<>0 \
         WHEN 'text' THEN json_extract({text},{path})<>'' \
         WHEN 'array' THEN EXISTS(SELECT 1 FROM json_each({text},{path})) \
         WHEN 'object' THEN EXISTS(SELECT 1 FROM json_each({text},{path})) \
         ELSE 0 END ELSE 0 END"
    )
}

/// Whether either stored blob says the task is a workflow builder.
pub fn workflow_builder() -> String {
    format!(
        "SELECT ({}) OR ({}) FROM task_headers h WHERE h.id=?1",
        builder_of("h.header"),
        builder_of("h.status")
    )
}

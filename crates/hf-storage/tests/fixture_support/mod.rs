#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared fixtures for the fixture-writer tests. Expected values are literals computed
//! outside the storage code that stores them back.

use crate::common::{self, Fixture, default_limits};
use hf_domain::{
    event_read::{EventFilter, EventQuery},
    process::ProcessIdentity,
    task_read::{Coverage, TaskId},
};
use hf_storage::{
    FixtureFacts, FixtureLine, FixturePrompt, FixtureTask, ReadSession, Store, TaskFixture,
};
use rusqlite::{Connection, OpenFlags, Params};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub const MAIN: &str = "store.sqlite3";
/// 2026-01-10T10:00:00Z and 2026-01-10T10:05:00Z in microseconds since the epoch.
pub const STARTED_US: i64 = 1_768_039_200_000_000;
pub const FINISHED_US: i64 = 1_768_039_500_000_000;

pub fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

pub fn tid(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

pub struct World {
    pub scratch: Fixture,
    pub root: PathBuf,
    pub store: Store,
}

/// A real initialized root holding an empty, fully covered task dataset.
pub fn world() -> World {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    let all = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: Vec::new(),
                launches: Vec::new(),
                coverage: all,
            },
            later(5),
        )
        .unwrap();
    World {
        scratch,
        root,
        store,
    }
}

impl World {
    /// A read-only connection next to the store's own, so closing it checkpoints nothing.
    pub fn db(&self) -> Connection {
        Connection::open_with_flags(self.root.join(MAIN), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    /// A writable plain connection, for statements the tests expect the schema to refuse.
    pub fn raw(&self) -> Connection {
        Connection::open(self.root.join(MAIN)).unwrap()
    }

    pub fn session(&self) -> ReadSession {
        ReadSession::open(
            &self.root,
            default_limits(),
            later(5),
            common::lease::lease_port(),
        )
        .unwrap()
    }

    pub fn add(&self, task: &FixtureTask) {
        self.store.add_task_fixture(task, later(5)).unwrap();
    }

    pub fn append(&self, id: &str, line: &FixtureLine) {
        self.store
            .append_event_record_fixture(&tid(id), line, later(5))
            .unwrap();
    }

    pub fn generation(&self) -> String {
        self.db()
            .query_row("SELECT generation FROM task_dataset", [], |r| r.get(0))
            .unwrap()
    }

    pub fn watch(&self) -> i64 {
        self.db()
            .query_row("SELECT revision FROM task_watch_meta", [], |r| r.get(0))
            .unwrap()
    }
}

pub fn one<T: rusqlite::types::FromSql>(db: &Connection, sql: &str, params: impl Params) -> T {
    db.query_row(sql, params, |r| r.get(0)).unwrap()
}

pub fn count(db: &Connection, table: &str, key: &str, id: &str) -> i64 {
    one(
        db,
        &format!("SELECT count(*) FROM {table} WHERE {key}=?1"),
        [id],
    )
}

pub fn is_hex32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn facts(id: &str, status: &str, exit_code: Option<i64>, drained: bool) -> FixtureFacts {
    let finished = status != "running";
    FixtureFacts {
        id: tid(id),
        backend: "claude".into(),
        repo_path: "/workspace/repo".into(),
        session_id: format!("session-{}", &id[..8]),
        status: status.into(),
        exit_code,
        started_at: "2026-01-10T10:00:00+00:00".into(),
        started_at_us: STARTED_US,
        finished_at: finished.then(|| "2026-01-10T10:05:00+00:00".to_owned()),
        finished_at_us: finished.then_some(FINISHED_US),
        output_drained: drained,
        title: Some("First task".into()),
        model: None,
        continuation_of: None,
        chain_root: None,
    }
}

pub fn inline_prompt(text: &str) -> FixturePrompt {
    FixturePrompt {
        inline_text: Some(text.into()),
        preview: None,
        claimed_sha256: None,
        source_name: None,
        blob: None,
    }
}

pub fn task(id: &str, status: &str, exit_code: Option<i64>, drained: bool) -> FixtureTask {
    FixtureTask {
        facts: facts(id, status, exit_code, drained),
        header: format!("header:{id}").into_bytes(),
        status: format!("status:{id}").into_bytes(),
        prompt: inline_prompt("Summarize the repo."),
        process: None,
    }
}

pub fn done(id: &str) -> FixtureTask {
    task(id, "completed", Some(0), true)
}

pub fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        1_768_000_000,
        123_456,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

pub fn event(seq: u64, kind: &str) -> FixtureLine {
    FixtureLine::Event {
        seq,
        kind: kind.into(),
        body: event_body(seq, kind),
    }
}

pub fn event_body(seq: u64, kind: &str) -> Vec<u8> {
    format!("{{\"seq\":{seq},\"kind\":\"{kind}\"}}").into_bytes()
}

/// Bytes a stored event line takes: the body and its newline.
pub fn event_len(seq: u64, kind: &str) -> i64 {
    event_body(seq, kind).len() as i64 + 1
}

pub const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub const C: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/// The named columns of the one row whose `key` is `id`, each rendered with SQLite's
/// `quote()` and joined by `|`, so integer, text, blob and NULL cells stay distinct.
pub fn quoted(db: &Connection, table: &str, columns: &str, key: &str, id: &str) -> String {
    let list: Vec<String> = columns.split(',').map(|c| format!("quote({c})")).collect();
    one(
        db,
        &format!(
            "SELECT {} FROM {table} WHERE {key}=?1",
            list.join("||'|'||")
        ),
        [id],
    )
}

pub fn hex(bytes: &[u8]) -> String {
    format!(
        "X'{}'",
        bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
    )
}

pub const FACT_COLUMNS: &str = "valid,backend,repo_path,session_id,status,exit_code,started_at,started_at_us,finished_at,finished_at_us,output_drained,title,model,continuation_of,chain_root";

pub const STREAM: &str =
    "complete_end,partial_tail,available,capture,drained,eof_seq,grace,task_status";

pub fn stream(w: &World, id: &str) -> String {
    quoted(&w.db(), "event_streams", STREAM, "task", id)
}

pub fn stream_generation(w: &World, id: &str) -> String {
    one(
        &w.db(),
        "SELECT generation FROM event_streams WHERE task=?1",
        [id],
    )
}

/// `start|end|class|seq|kind|body` of every record of the task, in physical order.
pub fn records(w: &World, id: &str) -> Vec<String> {
    let db = w.db();
    let mut statement = db
        .prepare("SELECT quote(start)||'|'||quote(end)||'|'||quote(class)||'|'||quote(seq)||'|'||quote(kind)||'|'||length(body) FROM event_records WHERE task=?1 ORDER BY start")
        .unwrap();
    statement
        .query_map([id], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

pub fn read(
    w: &World,
    id: &str,
    from_oldest: bool,
    cursor: Option<hf_domain::event_read::EventCursor>,
) -> hf_storage::Result<hf_application::event_read::EventReadResult> {
    w.session().event_read(
        &EventQuery {
            task: tid(id),
            limit: 200,
            filter: EventFilter::Default,
            from_oldest,
            cursor,
        },
        later(5),
    )
}

pub fn artifact_item(id: &str, path: &str) -> hf_storage::FixtureArtifact {
    hf_storage::FixtureArtifact {
        artifact_id: id.into(),
        path: path.into(),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-10T10:04:00+00:00".into(),
        provenance_method: "verified_process_lineage".into(),
        device: 1,
        inode: 2,
        size: 3,
        mtime_ns: 4,
    }
}

/// Every row of the table.
pub fn total(db: &Connection, table: &str) -> i64 {
    one(db, &format!("SELECT count(*) FROM {table}"), [])
}

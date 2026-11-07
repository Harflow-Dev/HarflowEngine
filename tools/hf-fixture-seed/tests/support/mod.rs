#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared helpers for the seed-tool process tests. The tool runs as a real child process
//! with the environment exactly `PATH=/usr/bin:/bin`; reads go through `hf-storage` in
//! this process. Expected values are literals written out here, not taken from the tool.

use hf_application::{event_read::EventReadResult, task_read::TaskReadResult};
use hf_domain::{
    event_read::{EventCursor, EventFilter, EventQuery},
    task_read::{AuthorizedScope, Cursor, PageFilter, TaskId, TaskQuery},
};
use hf_storage::{Limits, ReadSession};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

pub const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub const C: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
pub const D: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
pub const E: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
pub const MISSING: &str = "99999999-9999-4999-8999-999999999999";
/// Microseconds since the epoch of 2026-01-10T10:00:00Z, and one day's worth of them.
pub const A_STAMP: i64 = 1_768_039_200_000_000;
pub const DAY: i64 = 86_400_000_000;

/// Tests in one binary take turns: a child spawned while another test holds a lease can
/// inherit the open file description and make the lease look busy.
static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Scratch {
    pub base: PathBuf,
    pub root: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("hf-seed-test-{}-{n}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let base = std::fs::canonicalize(base).unwrap();
        let root = base.join("state");
        Self { base, root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(root: &Path, operation: &str, body: &str) -> Out {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hf-fixture-seed"))
        .arg(operation)
        .arg("--root")
        .arg(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Out {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Runs an operation that must succeed and returns its single stdout line.
pub fn ok(root: &Path, operation: &str, body: &str) -> String {
    let out = run(root, operation, body);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (Some(0), ""),
        "{operation}"
    );
    assert!(
        out.stdout.ends_with('\n') && out.stdout.matches('\n').count() == 1,
        "{:?}",
        out.stdout
    );
    out.stdout
}

pub fn digest(root: &Path) -> String {
    ok(root, "authoritative_digest", "{}")
        .trim()
        .trim_matches('"')
        .to_owned()
}

/// A JSON string literal.
pub fn esc(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn object(fields: &[(String, String)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{v}", esc(k)))
        .collect();
    format!("{{{}}}", parts.join(","))
}

/// A complete `add_task` spec; `over` entries hold raw JSON values and win over the defaults.
pub fn task(id: &str, over: &[(&str, &str)]) -> String {
    let mut fields: Vec<(String, String)> = [
        ("task_id", esc(id)),
        ("backend", esc("claude")),
        ("repo_path", esc("/workspace/repo")),
        ("freedom", esc("write_in_repo")),
        ("session_id", esc(&format!("session-{}", &id[..8]))),
        ("status", esc("completed")),
        ("exit_code", "0".into()),
        ("started_at", esc("2026-01-10T10:00:00+00:00")),
        ("finished_at", esc("2026-01-10T10:05:00+00:00")),
        ("prompt", esc("Summarize the repo.")),
        ("output_drained", "true".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    for (key, value) in over {
        match fields.iter_mut().find(|(k, _)| k == key) {
            Some(field) => field.1 = (*value).to_owned(),
            None => fields.push(((*key).to_owned(), (*value).to_owned())),
        }
    }
    object(&fields)
}

pub fn event(seq: u64, kind: &str, observed_at: &str, fields: &str) -> String {
    format!(
        "{{\"seq\":{seq},\"kind\":{},\"observed_at\":{},\"fields\":{fields}}}",
        esc(kind),
        esc(observed_at)
    )
}

pub fn event_for(id: &str, seq: u64, kind: &str, fields: &str) -> String {
    format!(
        "{{\"task_id\":{},\"seq\":{seq},\"kind\":{},\"observed_at\":\"2026-01-10T10:06:00+00:00\",\"fields\":{fields}}}",
        esc(id),
        esc(kind)
    )
}

fn a_events() -> Vec<String> {
    let t = |m: u32| format!("2026-01-10T10:0{m}:00+00:00");
    vec![
        event(0, "task_started", &t(0), r#"{"backend":"claude"}"#),
        event(1, "assistant_delta", &t(1), r#"{"text":"Sum"}"#),
        event(2, "assistant_delta", &t(1), r#"{"text":"marizing"}"#),
        event(
            3,
            "assistant_text",
            &t(2),
            r#"{"text":"Summarizing the repository."}"#,
        ),
        event(
            4,
            "tool_call",
            &t(3),
            r#"{"call_id":"call-1","name":"Read","input":{"path":"README.md"}}"#,
        ),
        event(
            5,
            "tool_result",
            &t(3),
            r#"{"call_id":"call-1","is_error":false,"output":"Readme body"}"#,
        ),
        event(
            6,
            "usage",
            &t(4),
            r#"{"input_tokens":120,"output_tokens":30}"#,
        ),
        event(
            7,
            "task_finished",
            &t(5),
            r#"{"status":"completed","exit_code":0}"#,
        ),
    ]
}

fn b_events() -> Vec<String> {
    let t = |m: u32| format!("2026-01-11T10:0{m}:00+00:00");
    vec![
        event(0, "task_started", &t(0), r#"{"backend":"claude"}"#),
        event(1, "assistant_text", &t(1), r#"{"text":"Starting."}"#),
        event(2, "notice", &t(2), r#"{"text":"stream ended"}"#),
    ]
}

pub fn long_prompt() -> String {
    (0..120)
        .map(|n| format!("line {n:04} é spaced   text\n"))
        .collect()
}

/// The conformance world: tasks A to E, events for A and B.
pub fn world() -> String {
    let long = esc(&long_prompt());
    let tasks = [
        task(
            A,
            &[
                ("title", "\"First task\""),
                ("group", "\"g1\""),
                ("model", "\"model-a\""),
            ],
        ),
        task(
            B,
            &[
                ("started_at", "\"2026-01-11T10:00:00+00:00\""),
                ("finished_at", "\"2026-01-11T10:03:00+00:00\""),
                ("status", "\"failed\""),
                ("exit_code", "1"),
                ("prompt", "\"Fix the bug.\""),
                ("title", "\"Second task\""),
            ],
        ),
        task(
            C,
            &[
                ("started_at", "\"2026-01-12T10:00:00+00:00\""),
                ("finished_at", "\"2026-01-12T10:02:00+00:00\""),
                ("continuation_of", &esc(A)),
                ("prompt", "\"Continue.\""),
                ("title", "\"Third task\""),
            ],
        ),
        task(
            D,
            &[
                ("started_at", "\"2026-01-13T10:00:00+00:00\""),
                ("finished_at", "\"2026-01-13T10:02:00+00:00\""),
                ("prompt", &long),
                ("prompt_store", "\"sidecar_verified\""),
            ],
        ),
        task(
            E,
            &[
                ("started_at", "\"2026-01-14T10:00:00+00:00\""),
                ("finished_at", "\"2026-01-14T10:02:00+00:00\""),
                ("prompt", &long),
                ("prompt_store", "\"sidecar_missing\""),
            ],
        ),
    ];
    format!(
        "{{\"world\":{{\"tasks\":[{}],\"events\":{{{}:[{}],{}:[{}]}}}}}}",
        tasks.join(","),
        esc(A),
        a_events().join(","),
        esc(B),
        b_events().join(",")
    )
}

pub fn seeded() -> Scratch {
    let scratch = Scratch::new();
    assert_eq!(ok(&scratch.root, "seed_world", &world()), "null\n");
    scratch
}

pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

pub fn tid(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

/// Opens a read session with the production lease, runs `f`, and closes it before returning.
pub fn read<T>(root: &Path, f: impl FnOnce(&ReadSession) -> T) -> T {
    let session = ReadSession::open(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    f(&session)
}

pub fn try_open(root: &Path) -> hf_storage::Result<ReadSession> {
    ReadSession::open(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
}

pub fn page(
    root: &Path,
    limit: usize,
    active_only: bool,
    session: Option<&str>,
    cursor: Option<Cursor>,
) -> TaskReadResult {
    let filter = PageFilter::new(active_only, session.map(str::to_owned)).unwrap();
    let query = TaskQuery::page(limit, filter, cursor).unwrap();
    read(root, |s| {
        s.task_read(&AuthorizedScope::Global, &query, deadline())
            .unwrap()
    })
}

pub fn lookup(root: &Path, ids: &[&str]) -> TaskReadResult {
    let query = TaskQuery::ids(ids.iter().map(|id| tid(id)).collect()).unwrap();
    read(root, |s| {
        s.task_read(&AuthorizedScope::Global, &query, deadline())
            .unwrap()
    })
}

pub fn events_with(
    root: &Path,
    id: &str,
    limit: usize,
    from_oldest: bool,
    cursor: Option<EventCursor>,
) -> hf_storage::Result<EventReadResult> {
    let query = EventQuery {
        task: tid(id),
        limit,
        filter: EventFilter::Default,
        from_oldest,
        cursor,
    };
    read(root, |s| s.event_read(&query, deadline()))
}

pub fn events(root: &Path, id: &str) -> EventReadResult {
    events_with(root, id, 200, true, None).unwrap()
}

pub fn seqs(result: &EventReadResult) -> Vec<u64> {
    result.events.iter().filter_map(|e| e.seq).collect()
}

pub fn ids(result: &TaskReadResult) -> Vec<&str> {
    result.rows.iter().map(|r| r.id.as_str()).collect()
}

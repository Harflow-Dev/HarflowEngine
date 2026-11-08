#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Roots and `harflow-ctl` runs for the store-backed read tests.

use hf_domain::task_read::{Coverage, TaskId};
use hf_storage::{FixtureFacts, FixturePrompt, FixtureTask, Limits, Store, TaskFixture};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn scratch(name: &str) -> Scratch {
    let base = std::env::temp_dir().join(format!("hf-ctl-reads-{name}-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    Scratch(fs::canonicalize(&base).unwrap())
}

pub fn later() -> Instant {
    Instant::now() + Duration::from_secs(120)
}

pub fn id(n: u32) -> TaskId {
    TaskId::new(format!("{n:08x}-0000-4000-8000-000000000000")).unwrap()
}

pub fn task(n: u32, status: &str, exit: Option<i64>, header_pad: usize, repo: &str) -> FixtureTask {
    let ident = id(n);
    let header = format!(
        r#"{{"task_id":"{}","group":"{}"}}"#,
        ident.as_str(),
        "t".repeat(header_pad)
    );
    let finished = exit.map(|_| "2026-01-01T00:00:01+00:00".to_string());
    FixtureTask {
        facts: FixtureFacts {
            id: ident.clone(),
            backend: "claude".into(),
            repo_path: repo.into(),
            session_id: "s".into(),
            status: status.into(),
            exit_code: exit,
            started_at: "2026-01-01T00:00:00+00:00".into(),
            started_at_us: 1_767_225_600_000_000 + i64::from(n) * 1_000_000,
            finished_at: finished.clone(),
            finished_at_us: finished.map(|_| 1_767_225_601_000_000 + i64::from(n) * 1_000_000),
            output_drained: true,
            title: None,
            model: None,
            continuation_of: None,
            chain_root: None,
        },
        header: header.into_bytes(),
        status: format!(r#"{{"task_id":"{}"}}"#, ident.as_str()).into_bytes(),
        prompt: FixturePrompt {
            inline_text: Some("p".into()),
            preview: None,
            claimed_sha256: None,
            source_name: None,
            blob: None,
        },
        process: None,
    }
}

/// The process-wide shared lease admits one open root at a time.
pub static ONE_ROOT: Mutex<()> = Mutex::new(());

pub fn exclusive() -> MutexGuard<'static, ()> {
    ONE_ROOT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn store(root: &Path) -> Store {
    let store = Store::initialize_fixture(
        root,
        Limits::default(),
        later(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let all = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let empty = TaskFixture {
        rows: Vec::new(),
        pending: Vec::new(),
        coverage: all,
    };
    store.replace_task_fixture(&empty, later()).unwrap();
    store
}

pub fn ctl(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_harflow-ctl"))
        .args(args)
        .arg("--json")
        .env("HARFLOW_STATE_ROOT", root)
        .env_remove("HARFLOW_TASK_ID")
        .env_remove("HARFLOW_DEPTH")
        .env_remove("PB_TASK_ID")
        .env_remove("PB_DEPTH")
        .output()
        .unwrap()
}

/// The document with the whitespace between its tokens removed: the transport writes Python's
/// default separators, and these tests read values, not spacing.
pub fn compact_json(document: &str) -> String {
    let mut out = String::with_capacity(document.len());
    let (mut quoted, mut escaped) = (false, false);
    for ch in document.chars() {
        if quoted {
            out.push(ch);
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
        } else if !ch.is_whitespace() {
            quoted = ch == '"';
            out.push(ch);
        }
    }
    out
}

pub fn text(output: &Output) -> String {
    let _ = &output.stderr;
    compact_json(&String::from_utf8(output.stdout.clone()).unwrap())
}

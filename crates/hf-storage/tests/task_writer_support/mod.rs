#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared helpers for the production task writer tests: a `TaskCreate` built from the
//! fixture defaults, and the publication state a write may change.

use crate::fixture_support::{World, facts, later, one};
use hf_storage::{TaskCreate, TaskProcess};

/// A completed task with the fixture facts, served blobs and the given prompt.
pub fn create(id: &str, status: &str, exit_code: Option<i64>, prompt: &str) -> TaskCreate {
    TaskCreate {
        facts: facts(id, status, exit_code, status != "running"),
        header: format!("header:{id}").into_bytes(),
        status: format!("status:{id}").into_bytes(),
        prompt: prompt.to_owned(),
        process: None,
    }
}

pub fn done(id: &str) -> TaskCreate {
    create(id, "completed", Some(0), "Summarize the repo.")
}

pub fn running(id: &str) -> TaskCreate {
    create(id, "running", None, "Summarize the repo.")
}

pub fn process(pid: u32) -> TaskProcess {
    TaskProcess {
        pid,
        start: Some(crate::fixture_support::identity(pid)),
    }
}

pub fn put(w: &World, task: &TaskCreate) {
    w.store.create_task(task, later(5)).unwrap();
}

pub fn sequence(w: &World) -> i64 {
    one(
        &w.db(),
        "SELECT value FROM commit_sequence WHERE singleton=1",
        [],
    )
}

/// Generation, watch revision and commit sequence together.
pub fn published(w: &World) -> (String, i64, i64) {
    (w.generation(), w.watch(), sequence(w))
}

pub const PROMPT_COLUMNS: &str =
    "form,inline_text,preview,claimed_sha256,source_name,truncated,blob";
pub const HEADER_COLUMNS: &str =
    "stamp,session,active,recorded_status,exited,drained,owned,header,status";
pub const PROCESS_COLUMNS: &str = "pid,start_seconds,start_micros,boot_uuid,pgid";

/// Every stored column of the task's rows in the four task tables, `NONE` for a missing row.
pub fn rows(w: &World, id: &str) -> Vec<String> {
    let db = w.db();
    [
        ("task_facts", crate::fixture_support::FACT_COLUMNS, "id"),
        ("task_headers", HEADER_COLUMNS, "id"),
        ("task_prompts", PROMPT_COLUMNS, "task"),
        ("task_processes", PROCESS_COLUMNS, "task"),
    ]
    .into_iter()
    .map(|(table, columns, key)| {
        if crate::fixture_support::count(&db, table, key, id) == 0 {
            "NONE".to_owned()
        } else {
            crate::fixture_support::quoted(&db, table, columns, key, id)
        }
    })
    .collect()
}

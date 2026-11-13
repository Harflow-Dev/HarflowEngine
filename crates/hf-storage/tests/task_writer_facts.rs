#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::create_task` fact bounds: each text fact at its bound lands, one byte over is
//! refused as `LimitExceeded` with nothing published, and a process identity naming another
//! pid is refused. The fixture wrapper keeps accepting over-long facts.
mod common;
mod fixture_support;
mod task_writer_support;
use fixture_support::{A, B, World, later, world};
use hf_storage::{
    DEFAULT_MAX_BYTES, Error, FACT_TEXT_LIMIT, FixturePrompt, FixtureTask, PROMPT_PREVIEW_CHARS,
    REPO_PATH_LIMIT, TITLE_LIMIT, TaskCreate, TaskFacts, TaskProcess,
};
use task_writer_support::{done, process, published, put, rows};

type Field = fn(&mut TaskFacts, String);

const FIELDS: [(&str, usize, Field); 7] = [
    ("backend", FACT_TEXT_LIMIT, |f, t| f.backend = t),
    ("model", FACT_TEXT_LIMIT, |f, t| f.model = Some(t)),
    ("started_at", FACT_TEXT_LIMIT, |f, t| f.started_at = t),
    ("finished_at", FACT_TEXT_LIMIT, |f, t| {
        f.finished_at = Some(t)
    }),
    ("chain_root", FACT_TEXT_LIMIT, |f, t| f.chain_root = Some(t)),
    ("repo_path", REPO_PATH_LIMIT, |f, t| f.repo_path = t),
    ("title", TITLE_LIMIT, |f, t| f.title = Some(t)),
];

fn with(id: &str, set: Field, bytes: usize) -> TaskCreate {
    let mut t = done(id);
    // A two-byte character first, so the bound is in bytes rather than characters.
    let text = format!("\u{e9}{}", "x".repeat(bytes - 2));
    assert_eq!(text.len(), bytes);
    set(&mut t.facts, text);
    t
}

fn refused(w: &World, task: &TaskCreate) {
    let before = published(w);
    let error = w.store.create_task(task, later(5)).unwrap_err();
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    assert_eq!(published(w), before, "a refused create publishes nothing");
    assert_eq!(
        rows(w, task.facts.id.as_str()),
        ["NONE", "NONE", "NONE", "NONE"]
    );
}

#[test]
fn each_fact_text_at_its_bound_lands_and_one_byte_over_is_refused() {
    for (name, bound, set) in FIELDS {
        let w = world();
        refused(&w, &with(B, set, bound + 1));
        put(&w, &with(A, set, bound));
        assert_eq!(rows(&w, A)[0].matches('\u{e9}').count(), 1, "{name}");
    }
}

#[test]
fn the_title_bound_keeps_a_written_prompt_within_the_default_read_budget() {
    let name = 64 + ".prompt.".len() + 64 + ".txt".len();
    assert!(PROMPT_PREVIEW_CHARS * 4 + TITLE_LIMIT + name <= DEFAULT_MAX_BYTES);
}

#[test]
fn a_process_identity_naming_another_pid_is_refused() {
    let w = world();
    let mut t = done(A);
    t.process = Some(TaskProcess {
        pid: 4242,
        start: process(4243).start,
    });
    refused(&w, &t);
    t.process = Some(TaskProcess {
        pid: 4242,
        start: None,
    });
    put(&w, &t);
}

#[test]
fn the_fixture_wrapper_keeps_accepting_an_over_long_title() {
    let w = world();
    let t = with(A, |f, t| f.title = Some(t), TITLE_LIMIT + 1);
    let fixture = FixtureTask {
        facts: t.facts,
        header: t.header,
        status: t.status,
        prompt: FixturePrompt {
            inline_text: Some(t.prompt),
            preview: None,
            claimed_sha256: None,
            source_name: None,
            blob: None,
        },
        process: None,
    };
    w.store.add_task_fixture(&fixture, later(5)).unwrap();
}

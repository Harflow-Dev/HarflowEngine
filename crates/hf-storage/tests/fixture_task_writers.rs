#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `add_task_fixture` and `damage_task_fixture`: stored values, refusals, generation and
//! watch publication.
mod common;
mod fixture_support;
use fixture_support::{
    A, B, FACT_COLUMNS, STARTED_US, count, done, hex, identity, inline_prompt, is_hex32, later,
    one, quoted, task, tid, world,
};

use hf_domain::task_read::{AuthorizedScope, Lookup, TaskQuery};
use hf_storage::{Error, FixtureProcess, FixturePrompt};

#[test]
fn an_added_task_stores_every_typed_fact_as_given() {
    let w = world();
    let mut t = done(A);
    t.facts.model = Some("model-a".into());
    w.add(&t);
    assert_eq!(
        quoted(&w.db(), "task_facts", FACT_COLUMNS, "id", A),
        "1|'claude'|'/workspace/repo'|'session-aaaaaaaa'|'completed'|0|'2026-01-10T10:00:00+00:00'|1768039200000000|'2026-01-10T10:05:00+00:00'|1768039500000000|1|'First task'|'model-a'|NULL|NULL"
    );
}

#[test]
fn a_continuation_stores_both_links_and_a_running_task_has_no_finish() {
    let w = world();
    let mut t = task(B, "running", None, false);
    t.facts.continuation_of = Some(tid(A));
    t.facts.chain_root = Some(tid(A));
    w.add(&t);
    assert_eq!(
        quoted(&w.db(), "task_facts", FACT_COLUMNS, "id", B),
        "1|'claude'|'/workspace/repo'|'session-bbbbbbbb'|'running'|NULL|'2026-01-10T10:00:00+00:00'|1768039200000000|NULL|NULL|0|'First task'|NULL|'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'|'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'"
    );
}

#[test]
fn the_header_row_maps_the_facts_that_reads_filter_and_sort_on() {
    let w = world();
    w.add(&done(A));
    w.add(&task(B, "running", None, false));
    let cols = "stamp,session,active,recorded_status,exited,drained,owned";
    assert_eq!(
        quoted(&w.db(), "task_headers", cols, "id", A),
        "1768039200000000|'session-aaaaaaaa'|0|'completed'|1|1|0"
    );
    assert_eq!(
        quoted(&w.db(), "task_headers", cols, "id", B),
        "1768039200000000|'session-bbbbbbbb'|1|'running'|0|0|0"
    );
    let blobs = quoted(&w.db(), "task_headers", "header,status", "id", A);
    assert_eq!(
        blobs,
        format!(
            "{}|{}",
            hex(format!("header:{A}").as_bytes()),
            hex(format!("status:{A}").as_bytes())
        )
    );
}

#[test]
fn a_terminal_status_with_no_exit_code_is_not_exited() {
    let w = world();
    w.add(&task(A, "failed", None, false));
    let exited: bool = one(&w.db(), "SELECT exited FROM task_headers WHERE id=?1", [A]);
    assert!(!exited);
}

#[test]
fn the_read_path_serves_the_added_tasks_newest_first_with_their_blobs() {
    let w = world();
    let mut older = done(A);
    older.facts.started_at_us = STARTED_US - 1_000_000;
    w.add(&older);
    w.add(&done(B));
    let result = w
        .session()
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::page(
                10,
                hf_domain::task_read::PageFilter::new(false, None).unwrap(),
                None,
            )
            .unwrap(),
            later(5),
        )
        .unwrap();
    let ids: Vec<&str> = result.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [B, A]);
    assert_eq!(result.rows[0].header, format!("header:{B}").into_bytes());
    assert_eq!(
        result.rows[0].status,
        Some(format!("status:{B}").into_bytes())
    );
    assert_eq!(result.rows[0].stamp, STARTED_US);
    assert!(result.activity_complete);
}

#[test]
fn an_added_task_is_found_by_exact_id_and_by_prefix_in_an_initially_empty_store() {
    let w = world();
    let session = w.session();
    let ask = |text: &str| {
        session
            .task_read(
                &AuthorizedScope::Global,
                &TaskQuery::StoredStatus(tid(text)),
                later(5),
            )
            .unwrap()
            .lookups
    };
    assert_eq!(
        ask("aaaaaaaa"),
        [Lookup::Missing],
        "full coverage from initialization"
    );
    drop(session);
    w.add(&done(A));
    let session = w.session();
    let found = session
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::StoredStatus(tid("aaaaaaaa")),
            later(5),
        )
        .unwrap();
    assert_eq!(found.lookups, [Lookup::Found(tid(A))]);
    assert_eq!(found.rows.len(), 1);
}

#[test]
fn an_existing_id_is_refused_and_nothing_changes() {
    let w = world();
    w.add(&done(A));
    let before = (w.generation(), w.watch());
    let mut other = task(A, "failed", Some(1), false);
    other.prompt = inline_prompt("different");
    let error = w.store.add_task_fixture(&other, later(5)).unwrap_err();
    assert!(matches!(error, Error::FixtureConflict), "{error:?}");
    assert_eq!((w.generation(), w.watch()), before);
    assert_eq!(
        quoted(
            &w.db(),
            "task_facts",
            "status,exit_code,output_drained",
            "id",
            A
        ),
        "'completed'|0|1"
    );
    let text: String = one(
        &w.db(),
        "SELECT inline_text FROM task_prompts WHERE task=?1",
        [A],
    );
    assert_eq!(text, "Summarize the repo.");
}

#[test]
fn a_damaged_record_also_blocks_a_later_add_of_the_same_id() {
    let w = world();
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    let error = w.store.add_task_fixture(&done(A), later(5)).unwrap_err();
    assert!(matches!(error, Error::FixtureConflict), "{error:?}");
}

#[test]
fn damage_removes_only_the_header_and_process_rows_and_keeps_the_rest() {
    let w = world();
    let mut t = done(A);
    t.process = Some(FixtureProcess {
        pid: 4242,
        start: Some(identity(4242)),
    });
    w.add(&t);
    w.add(&done(B));
    w.append(A, &fixture_support::event(0, "task_started"));
    let item = fixture_support::artifact_item("art-1", "/workspace/repo/out/a.txt");
    w.store
        .replace_artifacts_fixture(&tid(A), true, &[item], later(5))
        .unwrap();
    let prompt = quoted(&w.db(), "task_prompts", "form,inline_text", "task", A);
    let stream = fixture_support::stream(&w, A);
    let records = fixture_support::records(&w, A);
    let revision: String = one(
        &w.db(),
        "SELECT revision FROM artifact_manifests WHERE task=?1",
        [A],
    );
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    let db = w.db();
    assert_eq!(count(&db, "task_headers", "id", A), 0);
    assert_eq!(count(&db, "task_processes", "task", A), 0);
    assert_eq!(
        quoted(&db, "task_facts", FACT_COLUMNS, "id", A),
        "0|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL|NULL"
    );
    assert_eq!(count(&db, "task_facts", "id", A), 1);
    assert_eq!(
        quoted(&db, "task_prompts", "form,inline_text", "task", A),
        prompt
    );
    assert_eq!(fixture_support::stream(&w, A), stream);
    assert_eq!(fixture_support::records(&w, A), records);
    assert_eq!(count(&db, "artifact_items", "task", A), 1);
    let kept: String = one(
        &db,
        "SELECT revision FROM artifact_manifests WHERE task=?1",
        [A],
    );
    assert_eq!(kept, revision);
    for (table, key) in [
        ("task_headers", "id"),
        ("task_prompts", "task"),
        ("task_facts", "id"),
    ] {
        assert_eq!(count(&db, table, key, B), 1, "other tasks keep {table}");
    }
}

#[test]
fn damaging_twice_is_idempotent_and_a_damaged_task_leaves_the_read_path() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    assert_eq!(count(&w.db(), "task_facts", "id", A), 1);
    let result = w
        .session()
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::Ids(vec![tid(A), tid(B)]),
            later(5),
        )
        .unwrap();
    assert_eq!(result.lookups, [Lookup::Missing, Lookup::Found(tid(B))]);
}

#[test]
fn damaging_an_unknown_id_creates_a_damaged_record_without_a_header() {
    let w = world();
    w.store.damage_task_fixture(&tid(C_ID), later(5)).unwrap();
    let valid: bool = one(&w.db(), "SELECT valid FROM task_facts WHERE id=?1", [C_ID]);
    assert!(!valid);
    assert_eq!(count(&w.db(), "task_headers", "id", C_ID), 0);
}

const C_ID: &str = fixture_support::C;

#[test]
fn the_dataset_generation_changes_on_every_task_write() {
    let w = world();
    let g0 = w.generation();
    assert!(is_hex32(&g0), "{g0}");
    w.add(&done(A));
    let g1 = w.generation();
    w.add(&done(B));
    let g2 = w.generation();
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    let g3 = w.generation();
    let all = [g0, g1, g2, g3];
    for generation in &all {
        assert!(is_hex32(generation), "{generation}");
    }
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn coverage_stays_complete_after_task_writes() {
    let w = world();
    w.add(&done(A));
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    let flags: (i64, i64, i64) = w
        .db()
        .query_row(
            "SELECT discovery,ownership,source FROM task_dataset",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(flags, (1, 1, 1));
}

#[test]
fn the_watch_revision_is_published_once_per_add_and_per_damage() {
    let w = world();
    let start = w.watch();
    w.add(&done(A));
    assert_eq!(w.watch(), start + 1);
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    assert_eq!(w.watch(), start + 2);
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    assert_eq!(w.watch(), start + 3);
    assert!(w.store.add_task_fixture(&done(B), later(5)).is_err());
    assert_eq!(w.watch(), start + 3, "a refused add publishes nothing");
}

#[test]
fn a_failure_inside_the_transaction_rolls_back_every_row() {
    let w = world();
    let before = (w.generation(), w.watch());
    let mut bad = done(A);
    bad.prompt = FixturePrompt {
        inline_text: Some("text".into()),
        preview: Some("conflicting preview".into()),
        claimed_sha256: None,
        source_name: None,
        blob: None,
    };
    assert!(w.store.add_task_fixture(&bad, later(5)).is_err());
    let db = w.db();
    for (table, key) in [
        ("task_facts", "id"),
        ("task_headers", "id"),
        ("task_prompts", "task"),
    ] {
        assert_eq!(count(&db, table, key, A), 0, "{table}");
    }
    assert_eq!((w.generation(), w.watch()), before);
}

#[test]
fn oversized_or_inconsistent_input_is_refused_before_any_write() {
    let w = world();
    let before = (w.generation(), w.watch());
    let mut cases = Vec::new();
    let mut t = done(A);
    t.header = vec![b'h'; 2049];
    cases.push(t);
    let mut t = done(A);
    t.status = vec![b's'; 2049];
    cases.push(t);
    let mut t = done(A);
    t.facts.status = "x".repeat(257);
    cases.push(t);
    let mut t = done(A);
    t.facts.finished_at_us = None;
    cases.push(t);
    let mut t = done(A);
    t.process = Some(FixtureProcess {
        pid: 1,
        start: None,
    });
    cases.push(t);
    let mut t = done(A);
    t.process = Some(FixtureProcess {
        pid: i32::MAX as u32 + 1,
        start: None,
    });
    cases.push(t);
    for t in &cases {
        let error = w.store.add_task_fixture(t, later(5)).unwrap_err();
        assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    }
    assert_eq!(count(&w.db(), "task_facts", "id", A), 0);
    assert_eq!((w.generation(), w.watch()), before);
}

#[test]
fn blobs_at_the_size_limit_are_accepted() {
    let w = world();
    let mut t = done(A);
    t.header = vec![b'h'; 2048];
    t.status = vec![b's'; 2048];
    w.add(&t);
    let len: i64 = one(
        &w.db(),
        "SELECT length(header)+length(status) FROM task_headers WHERE id=?1",
        [A],
    );
    assert_eq!(len, 4096);
}

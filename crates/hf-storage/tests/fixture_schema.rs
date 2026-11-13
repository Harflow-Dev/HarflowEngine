#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Schema 7: version pins, the additive tables, their CHECK constraints and the refusal
//! of a schema 6 root.
mod common;
mod fixture_support;
use fixture_support::{A, B, MAIN, World, count, done, later, one, quoted, tid, total, world};
use hf_storage::{ReadRefusal, ReadSession, SCHEMA_VERSION, Store};
use rusqlite::{Connection, ErrorCode};
use std::fs;

fn names(w: &World, kind: &str) -> Vec<String> {
    let db = w.db();
    let mut statement = db
        .prepare("SELECT name FROM sqlite_schema WHERE type=?1 AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    statement
        .query_map([kind], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn the_schema_is_version_seven_in_the_constant_the_marker_and_the_database() {
    assert_eq!(SCHEMA_VERSION, 7);
    let w = world();
    let version: i64 = one(&w.db(), "PRAGMA user_version", []);
    assert_eq!(version, 7);
    let marker = fs::read_to_string(w.root.join("store.identity")).unwrap();
    let lines: Vec<&str> = marker.lines().collect();
    assert_eq!((lines[0], lines[1]), ("harflow-storage-2", "7"));
    assert_eq!(w.store.identity().schema_version, 7);
}

#[test]
fn initialization_adds_the_five_fact_tables_and_their_indexes_to_the_older_ones() {
    let w = world();
    let tables = names(&w, "table");
    for table in [
        "task_facts",
        "task_processes",
        "task_prompts",
        "artifact_manifests",
        "artifact_items",
        "task_dataset",
        "task_headers",
        "launch_attempts",
        "chain_stops",
        "task_watch_meta",
        "event_dataset",
        "event_streams",
        "event_records",
        "store_identity",
    ] {
        assert!(tables.iter().any(|t| t == table), "{table} in {tables:?}");
    }
    let indexes = names(&w, "index");
    for index in ["facts_continuation", "facts_finished", "facts_started"] {
        assert!(indexes.iter().any(|i| i == index), "{index} in {indexes:?}");
    }
    assert_eq!(
        total(&w.db(), "task_facts"),
        0,
        "an empty root has no tasks"
    );
}

#[test]
fn a_schema_6_root_is_refused_by_the_writer_and_the_reader() {
    let w = world();
    let root = w.root.clone();
    let scratch = w.scratch;
    drop(w.store);
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    let generation = marker.lines().nth(2).unwrap().to_owned();
    fs::write(
        root.join("store.identity"),
        format!("harflow-storage-2\n6\n{generation}\n"),
    )
    .unwrap();
    let db = Connection::open(root.join(MAIN)).unwrap();
    db.execute_batch("PRAGMA user_version=6;").unwrap();
    drop(db);
    let _ = fs::remove_file(root.join("store.sqlite3-shm"));
    fs::write(root.join("store.sqlite3-wal"), b"").unwrap();

    let error = match ReadSession::open(
        &root,
        common::default_limits(),
        later(5),
        common::lease::lease_port(),
    ) {
        Ok(_) => panic!("a schema 6 root must be refused"),
        Err(error) => error,
    };
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::UnsupportedLayout);
    let refused = Store::open(
        &root,
        common::default_limits(),
        later(5),
        common::lease::lease_port(),
    );
    assert!(
        refused.is_err(),
        "the writer must refuse a schema 6 root too"
    );
    drop(scratch);
}

fn failing(w: &World, sql: &str) {
    let error = w.raw().execute_batch(sql).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation),
        "{sql}: {error}"
    );
}

const FACTS_HEAD: &str = "INSERT INTO task_facts(id,valid,backend,repo_path,session_id,status,started_at,started_at_us,output_drained,finished_at,finished_at_us,exit_code,title) VALUES";

#[test]
fn a_valid_fact_row_is_accepted_so_each_refusal_below_tests_one_defect() {
    let w = world();
    w.raw()
        .execute_batch(&format!(
            "{FACTS_HEAD}('ok',1,'b','/r','s','completed','t',1,1,'f',2,0,NULL)"
        ))
        .unwrap();
    assert_eq!(
        quoted(
            &w.db(),
            "task_facts",
            "valid,finished_at,finished_at_us,exit_code",
            "id",
            "ok"
        ),
        "1|'f'|2|0"
    );
    w.raw()
        .execute_batch(&format!(
            "{FACTS_HEAD}('run',1,'b','/r','s','running','t',1,0,NULL,NULL,NULL,NULL)"
        ))
        .unwrap();
    w.raw()
        .execute_batch(&format!(
            "{FACTS_HEAD}('bad',0,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL)"
        ))
        .unwrap();
}

#[test]
fn a_valid_row_missing_a_required_fact_is_refused() {
    let w = world();
    let base = ["'b'", "'/r'", "'s'", "'completed'", "'t'", "1", "1"];
    for i in 0..base.len() {
        let mut values = base.to_vec();
        values[i] = "NULL";
        failing(
            &w,
            &format!(
                "{FACTS_HEAD}('x{i}',1,{},NULL,NULL,NULL,NULL)",
                values.join(",")
            ),
        );
    }
}

#[test]
fn finish_text_and_finish_instant_must_both_be_present_or_both_absent() {
    let w = world();
    failing(
        &w,
        &format!("{FACTS_HEAD}('f1',1,'b','/r','s','completed','t',1,1,'f',NULL,NULL,NULL)"),
    );
    failing(
        &w,
        &format!("{FACTS_HEAD}('f2',1,'b','/r','s','completed','t',1,1,NULL,5,NULL,NULL)"),
    );
}

#[test]
fn flags_and_ids_outside_their_domain_are_refused() {
    let w = world();
    failing(
        &w,
        &format!("{FACTS_HEAD}('v2',2,'b','/r','s','completed','t',1,1,NULL,NULL,NULL,NULL)"),
    );
    failing(
        &w,
        &format!("{FACTS_HEAD}('d2',1,'b','/r','s','completed','t',1,2,NULL,NULL,NULL,NULL)"),
    );
    failing(
        &w,
        &format!("{FACTS_HEAD}('',1,'b','/r','s','completed','t',1,1,NULL,NULL,NULL,NULL)"),
    );
    let long = "x".repeat(65);
    failing(
        &w,
        &format!("{FACTS_HEAD}('{long}',1,'b','/r','s','completed','t',1,1,NULL,NULL,NULL,NULL)"),
    );
}

#[test]
fn a_damaged_row_must_have_every_other_column_null() {
    let w = world();
    failing(
        &w,
        "INSERT INTO task_facts(id,valid,backend) VALUES('d1',0,'b')",
    );
    failing(
        &w,
        "INSERT INTO task_facts(id,valid,exit_code) VALUES('d2',0,0)",
    );
    failing(
        &w,
        "INSERT INTO task_facts(id,valid,title) VALUES('d3',0,'t')",
    );
    failing(
        &w,
        "INSERT INTO task_facts(id,valid,chain_root) VALUES('d4',0,'r')",
    );
    failing(
        &w,
        "INSERT INTO task_facts(id,valid,output_drained) VALUES('d5',0,1)",
    );
}

#[test]
fn process_rows_need_a_real_pid_and_all_or_none_of_the_start_columns() {
    let w = world();
    w.raw()
        .execute_batch("INSERT INTO task_processes VALUES('ok',4242,1,2,'u',NULL)")
        .unwrap();
    w.raw()
        .execute_batch("INSERT INTO task_processes VALUES('none',4242,NULL,NULL,NULL,NULL)")
        .unwrap();
    for bad in [
        "INSERT INTO task_processes VALUES('p',1,1,2,'u',NULL)",
        "INSERT INTO task_processes VALUES('p',0,NULL,NULL,NULL,NULL)",
        "INSERT INTO task_processes VALUES('p',4242,1,NULL,NULL,NULL)",
        "INSERT INTO task_processes VALUES('p',4242,NULL,2,NULL,NULL)",
        "INSERT INTO task_processes VALUES('p',4242,NULL,NULL,'u',NULL)",
        "INSERT INTO task_processes VALUES('p',4242,1,2,NULL,NULL)",
    ] {
        failing(&w, bad);
    }
}

#[test]
fn prompt_rows_must_match_their_form() {
    let w = world();
    w.raw()
        .execute_batch(
            "INSERT INTO task_prompts VALUES('i',  'inline',  't',NULL,NULL,NULL,0,NULL)",
        )
        .unwrap();
    w.raw()
        .execute_batch("INSERT INTO task_prompts VALUES('s',  'sidecar',NULL,'p','h','n',1,NULL)")
        .unwrap();
    w.raw()
        .execute_batch("INSERT INTO task_prompts VALUES('s2', 'sidecar',NULL,'p',NULL,'n',1,x'00')")
        .unwrap();
    for bad in [
        "INSERT INTO task_prompts VALUES('b','inline','t','p',NULL,NULL,0,NULL)",
        "INSERT INTO task_prompts VALUES('b','inline',NULL,NULL,NULL,NULL,0,NULL)",
        "INSERT INTO task_prompts VALUES('b','inline','t',NULL,'h',NULL,0,NULL)",
        "INSERT INTO task_prompts VALUES('b','inline','t',NULL,NULL,'n',0,NULL)",
        "INSERT INTO task_prompts VALUES('b','inline','t',NULL,NULL,NULL,1,NULL)",
        "INSERT INTO task_prompts VALUES('b','inline','t',NULL,NULL,NULL,0,x'00')",
        "INSERT INTO task_prompts VALUES('b','sidecar','t','p','h','n',1,NULL)",
        "INSERT INTO task_prompts VALUES('b','sidecar',NULL,NULL,'h','n',1,NULL)",
        "INSERT INTO task_prompts VALUES('b','sidecar',NULL,'p','h',NULL,1,NULL)",
        "INSERT INTO task_prompts VALUES('b','sidecar',NULL,'p','h','n',0,NULL)",
        "INSERT INTO task_prompts VALUES('b','other',NULL,'p','h','n',1,NULL)",
        "INSERT INTO task_prompts VALUES('b','sidecar',NULL,'p','h','n',2,NULL)",
    ] {
        failing(&w, bad);
    }
}

#[test]
fn manifest_and_item_rows_enforce_their_domains() {
    let w = world();
    let revision = "0123456789abcdef0123456789abcdef";
    w.raw()
        .execute_batch(&format!(
            "INSERT INTO artifact_manifests VALUES('m',1,'{revision}')"
        ))
        .unwrap();
    failing(
        &w,
        &format!("INSERT INTO artifact_manifests VALUES('m2',2,'{revision}')"),
    );
    failing(&w, "INSERT INTO artifact_manifests VALUES('m3',1,'short')");
    failing(
        &w,
        &format!("INSERT INTO artifact_manifests VALUES('m4',1,'{revision}0')"),
    );
    let item = |ordinal: i64, size: i64| {
        format!(
            "INSERT INTO artifact_items VALUES('m',{ordinal},'id','/p','k','n','t','method','1','2',{size},'4',NULL,NULL,NULL,NULL,0)"
        )
    };
    w.raw().execute_batch(&item(0, 0)).unwrap();
    failing(&w, &item(-1, 0));
    failing(&w, &item(1, -1));
}

#[test]
fn replacing_the_task_fixture_clears_the_five_fact_tables() {
    let w = world();
    let mut t = done(A);
    t.process = Some(hf_storage::FixtureProcess {
        pid: 4242,
        start: None,
    });
    w.add(&t);
    w.add(&done(B));
    w.store
        .replace_artifacts_fixture(
            &tid(A),
            true,
            &[fixture_support::artifact_item("a1", "/p/a")],
            later(5),
        )
        .unwrap();
    for table in [
        "task_facts",
        "task_processes",
        "task_prompts",
        "artifact_manifests",
        "artifact_items",
    ] {
        assert!(total(&w.db(), table) > 0, "{table} is populated");
    }
    let fixture = hf_storage::TaskFixture {
        rows: Vec::new(),
        launches: Vec::new(),
        coverage: hf_domain::task_read::Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    };
    w.store.replace_task_fixture(&fixture, later(5)).unwrap();
    for table in [
        "task_facts",
        "task_processes",
        "task_prompts",
        "artifact_manifests",
        "artifact_items",
        "task_headers",
    ] {
        assert_eq!(total(&w.db(), table), 0, "{table}");
    }
    w.add(&done(A));
    assert_eq!(
        count(&w.db(), "task_facts", "id", A),
        1,
        "the id is free again"
    );
}

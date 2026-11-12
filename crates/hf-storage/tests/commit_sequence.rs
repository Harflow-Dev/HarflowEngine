#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `commit_sequence`: created at zero by initialize, advanced by exactly one per committed
//! public write, and untouched by rollbacks, failed publications and reads.
mod common;
mod fixture_support;
use common::{Fixture, deadline, default_limits};
use fixture_support::{A, B, World, artifact_item, done, event, later, tid, total, world};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_storage::{ChainStop, Error, EventFixture, LaunchRow, Mutation, Store};

fn sequence(w: &World) -> i64 {
    fixture_support::one(
        &w.db(),
        "SELECT value FROM commit_sequence WHERE singleton=1",
        [],
    )
}

/// Runs one write and asserts that it advanced the sequence by exactly one.
fn plus_one(w: &World, what: &str, write: impl FnOnce(&Store)) {
    let before = sequence(w);
    write(&w.store);
    assert_eq!(sequence(w), before + 1, "{what}");
}

fn record(start: u64, seq: u64) -> EventRecord {
    let body = format!(
        r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"record-{seq}"}}"#
    )
    .into_bytes();
    EventRecord {
        start,
        end: start + body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some("assistant_text".into()),
        body,
    }
}

fn event_fixture(records: Vec<EventRecord>) -> EventFixture {
    EventFixture {
        task: tid(A),
        records,
        partial_tail: 0,
        facts: LogFacts {
            task_status: Some("completed".into()),
            available: true,
            capture: CaptureState::Live,
            output_drained: None,
            eof_finished_seq: None,
            within_grace: false,
        },
    }
}

#[test]
fn initialize_creates_the_singleton_row_at_zero_and_refuses_a_second_row() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let db = rusqlite::Connection::open_with_flags(
        root.join("store.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let rows: (i64, i64, String) = db
        .query_row(
            "SELECT count(*),min(value),typeof(min(value)) FROM commit_sequence",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(rows, (1, 0, "integer".to_owned()));
    drop(db);
    let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    assert!(
        raw.execute("INSERT INTO commit_sequence VALUES(2,0)", [])
            .is_err()
    );
    drop(raw);
    drop(store);
}

#[test]
fn every_task_catalog_and_launch_writer_advances_the_sequence_by_exactly_one() {
    let w = world();
    assert_eq!(sequence(&w), 1, "the world's own replace");
    plus_one(&w, "add_task_fixture", |s| {
        s.add_task_fixture(&done(A), later(5)).unwrap()
    });
    plus_one(&w, "add second", |s| {
        s.add_task_fixture(&done(B), later(5)).unwrap()
    });
    plus_one(&w, "damage_task_fixture", |s| {
        s.damage_task_fixture(&tid(B), later(5)).unwrap()
    });
    plus_one(&w, "put_launch_attempt", |s| {
        s.put_launch_attempt(&LaunchRow::waiting_for_resources(tid(B)), later(5))
            .unwrap()
    });
    plus_one(&w, "replace_launch_fixture", |s| {
        s.replace_launch_fixture(&[LaunchRow::waiting_for_resources(tid(A))], later(5))
            .unwrap()
    });
    plus_one(&w, "put_chain_stop", |s| {
        s.put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
            .unwrap()
    });
    plus_one(&w, "remove_chain_stop", |s| {
        s.remove_chain_stop(&tid(A), later(5)).unwrap()
    });
    plus_one(&w, "remove_chain_stop on an absent root", |s| {
        s.remove_chain_stop(&tid(B), later(5)).unwrap()
    });
    plus_one(&w, "replace_task_fixture", |s| {
        s.replace_task_fixture(
            &hf_storage::TaskFixture {
                rows: Vec::new(),
                launches: Vec::new(),
                coverage: hf_domain::task_read::Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            later(5),
        )
        .unwrap()
    });
}

#[test]
fn every_event_prompt_artifact_and_batch_writer_advances_the_sequence_by_exactly_one() {
    let w = world();
    w.add(&done(A));
    plus_one(&w, "append_event_record_fixture", |s| {
        s.append_event_record_fixture(&tid(A), &event(0, "task_started"), later(5))
            .unwrap()
    });
    plus_one(&w, "truncate_event_log_fixture", |s| {
        s.truncate_event_log_fixture(&tid(A), 1, later(5)).unwrap()
    });
    plus_one(&w, "remove_event_log_fixture", |s| {
        s.remove_event_log_fixture(&tid(A), later(5)).unwrap()
    });
    let first = record(0, 0);
    let next = record(first.end, 1);
    plus_one(&w, "replace_event_fixture", |s| {
        s.replace_event_fixture(&event_fixture(vec![first]), later(5))
            .unwrap()
    });
    plus_one(&w, "append_event_fixture", |s| {
        s.append_event_fixture(&event_fixture(vec![next]), later(5))
            .unwrap()
    });
    plus_one(
        &w,
        "append_prompt_blob_fixture without a stored blob",
        |s| {
            assert!(
                !s.append_prompt_blob_fixture(&tid(A), b"x", later(5))
                    .unwrap()
            );
        },
    );
    plus_one(&w, "replace_artifacts_fixture", |s| {
        s.replace_artifacts_fixture(&tid(A), true, &[artifact_item("a1", "/p/a")], later(5))
            .unwrap()
    });
    plus_one(&w, "apply_batch with several mutations", |s| {
        s.apply_batch(
            &[
                Mutation::Insert {
                    key: "one".into(),
                    value: vec![1],
                },
                Mutation::Put {
                    key: "one".into(),
                    value: vec![2],
                },
                Mutation::Delete { key: "one".into() },
            ],
            later(5),
        )
        .unwrap()
    });
    plus_one(&w, "apply_batch with no mutations", |s| {
        s.apply_batch(&[], later(5)).unwrap()
    });
}

#[test]
fn an_error_inside_the_action_leaves_the_sequence_and_the_rows_unchanged() {
    let w = world();
    w.add(&done(A));
    w.store
        .put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
        .unwrap();
    let before = sequence(&w);
    let revision = w.watch();
    assert!(matches!(
        w.store.add_task_fixture(&done(A), later(5)),
        Err(Error::FixtureConflict)
    ));
    assert!(
        w.store
            .put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
            .is_err()
    );
    assert!(matches!(
        w.store.remove_event_log_fixture(&tid(B), later(5)),
        Err(Error::FixtureMissing)
    ));
    assert!(matches!(
        w.store
            .replace_artifacts_fixture(&tid(B), true, &[], later(5)),
        Err(Error::FixtureMissing)
    ));
    let batch = [
        Mutation::Insert {
            key: "k".into(),
            value: vec![1],
        },
        Mutation::Insert {
            key: "k".into(),
            value: vec![2],
        },
    ];
    assert!(w.store.apply_batch(&batch, later(5)).is_err());
    assert_eq!(sequence(&w), before);
    assert_eq!(w.watch(), revision);
    assert_eq!(total(&w.db(), "fixture_records"), 0, "the batch is atomic");
}

#[test]
fn a_failed_publication_rolls_back_the_action_and_the_sequence() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let db = || {
        rusqlite::Connection::open_with_flags(
            root.join("store.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    };
    // No task dataset has been published, so every generation rotation refuses.
    assert!(matches!(
        store.add_task_fixture(&done(A), later(5)),
        Err(Error::TaskReadUnavailable)
    ));
    assert_eq!(
        total(&db(), "task_facts"),
        0,
        "the inserted task row is rolled back with the failed publication"
    );
    assert_eq!(total(&db(), "task_headers"), 0);
    assert!(matches!(
        store.damage_task_fixture(&tid(A), later(5)),
        Err(Error::TaskReadUnavailable)
    ));
    assert!(matches!(
        store.put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5)),
        Err(Error::TaskReadUnavailable)
    ));
    assert!(matches!(
        store.replace_launch_fixture(&[], later(5)),
        Err(Error::TaskReadUnavailable)
    ));
    let value: i64 = db()
        .query_row("SELECT value FROM commit_sequence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, 0);
    for table in ["task_headers", "task_facts", "launch_attempts"] {
        assert_eq!(total(&db(), table), 0, "{table}");
    }
}

#[test]
fn reads_never_change_the_sequence() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.store
        .apply_batch(
            &[Mutation::Put {
                key: "r".into(),
                value: vec![1],
            }],
            later(5),
        )
        .unwrap();
    let before = sequence(&w);
    let session = w.session();
    fixture_support::read(&w, A, true, None).unwrap();
    assert_eq!(
        w.store.read_batch(&["r".into()], later(5)).unwrap().len(),
        1
    );
    drop(session);
    let again = w.session();
    drop(again);
    assert_eq!(sequence(&w), before);
}

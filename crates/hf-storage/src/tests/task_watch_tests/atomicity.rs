#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "actors.rs"]
mod actors;
use super::support::*;
use actors::StageActor;
use hf_application::task_watch::TaskWatchPort;
use hf_domain::task_watch::WatchState;
use rusqlite::Connection;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
#[test]
fn revision_overflow_aborts_headers_and_watch_revision_in_one_transaction() {
    let fixture = Fixture::new("overflow");
    let writer = fixture.initialize();
    publish(&writer, &[("stable", b"before")]);
    drop(writer);
    let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
    raw.execute("UPDATE task_watch_meta SET revision=?1", [i64::MAX])
        .unwrap();
    drop(raw);
    let writer = fixture.open();
    assert!(matches!(
        writer.replace_task_fixture(&task_fixture(&[("stable", b"after")]), deadline()),
        Err(crate::Error::TaskWatchUnavailable)
    ));
    let raw = fixture.raw();
    assert_eq!(read_revision(&raw), i64::MAX);
    assert_eq!(
        raw.query_row(
            "SELECT header FROM task_headers WHERE id='stable'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        b"before"
    );
}

#[test]
fn killed_task_catalog_writer_is_atomic_on_both_sides_of_commit_and_later_writer_survives() {
    for (stage, expected_revision, expected_header) in [
        ("before_task_catalog_commit", 1, b"before".as_slice()),
        ("after_task_catalog_commit", 2, b"after".as_slice()),
    ] {
        let fixture = Fixture::new("crash-boundary");
        let store = fixture.initialize();
        publish(&store, &[("atomic", b"before")]);
        drop(store);
        let mut actor = StageActor::spawn(&fixture.root(), stage, None);
        actor.wait_at_stage(&format!("STAGE:{stage}"));
        let raw = fixture.raw();
        assert_eq!(read_revision(&raw), expected_revision);
        assert_eq!(
            raw.query_row(
                "SELECT header FROM task_headers WHERE id='atomic'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
            expected_header
        );
        drop(raw);
        actor.kill_and_reap();
        let resumed = fixture.open();
        publish(&resumed, &[("atomic", b"survivor")]);
        drop(resumed);
        let verify = fixture.raw();
        assert_eq!(read_revision(&verify), expected_revision + 1);
        assert_eq!(
            verify
                .query_row(
                    "SELECT header FROM task_headers WHERE id='atomic'",
                    [],
                    |row| row.get::<_, Vec<u8>>(0)
                )
                .unwrap(),
            b"survivor"
        );
    }
}

#[test]
fn wait_releases_store_admission_before_wait_callback_publishes() {
    let fixture = Fixture::new("wait-unlocked");
    let writer = Arc::new(Mutex::new(fixture.initialize()));
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let clock = Clock::default();
    let actor = writer.clone();
    clock.on_wait(move || {
        let store = actor.lock().unwrap();
        publish(&store, &[("during-wait", b"committed")]);
    });
    let result = watch(&fixture.root(), clock)
        .execute(&request(Some(baseline), 2), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Changed);
    assert_eq!(read_revision(&fixture.raw()), 1);
}

#[test]
fn deadline_expiring_after_dml_rolls_back_metadata_and_headers_before_commit() {
    let fixture = Fixture::new("expired-before-commit");
    let store = fixture.initialize();
    publish(&store, &[("atomic", b"before")]);
    drop(store);
    let mut actor = StageActor::spawn(&fixture.root(), "before_task_catalog_commit", Some(100));
    actor.wait_at_stage("STAGE:before_task_catalog_commit");
    assert!(
        actor.marker_absent_for(Duration::from_millis(300)),
        "actor remains stopped at the injected pre-commit boundary until released"
    );
    actor.release_stage();
    actor.wait_marker("WATCH_STAGE_RESULT:deadline-rollback");
    actor.wait_success();
    let raw = fixture.raw();
    assert_eq!(read_revision(&raw), 1);
    assert_eq!(
        raw.query_row(
            "SELECT header FROM task_headers WHERE id='atomic'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        b"before"
    );
}

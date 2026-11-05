#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::support::*;
use hf_application::task_watch::TaskWatchPort;
use hf_domain::{
    task_read::TaskId,
    task_watch::{WatchReason, WatchState},
};
use rusqlite::Connection;
#[test]
fn malformed_revision_metadata_is_unavailable_and_never_repaired() {
    for malformed in [
        "UPDATE task_watch_meta SET revision=-1",
        "UPDATE task_watch_meta SET version=2",
        "DROP TABLE task_watch_meta",
    ] {
        let fixture = Fixture::new("malformed-meta");
        let store = fixture.initialize();
        let baseline = watch(&fixture.root(), Clock::default())
            .execute(&request(None, 0), deadline())
            .unwrap()
            .cursor
            .unwrap();
        drop(store);
        let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
        raw.execute_batch(malformed).unwrap();
        drop(raw);
        let result = watch(&fixture.root(), Clock::default())
            .execute(&request(Some(baseline), 0), deadline())
            .unwrap();
        assert_eq!(result.state, WatchState::Unavailable, "{malformed}");
        assert_eq!(result.reason, Some(WatchReason::ObservationUnavailable));
        if malformed.starts_with("UPDATE task_watch_meta SET revision") {
            let verify = fixture.raw();
            let value: i64 = verify
                .query_row(
                    "SELECT revision FROM task_watch_meta WHERE singleton=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                value, -1,
                "watch observation must not repair malformed metadata"
            );
        } else if malformed.starts_with("UPDATE task_watch_meta SET version") {
            let verify = fixture.raw();
            let version: i64 = verify
                .query_row(
                    "SELECT version FROM task_watch_meta WHERE singleton=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                version, 2,
                "watch observation must not repair malformed metadata"
            );
        }
    }
}

#[test]
fn future_version_extra_rows_and_large_scalar_cells_are_unavailable_without_repair() {
    for schema in [
        "UPDATE task_watch_meta SET version=9",
        "DROP TABLE task_watch_meta; CREATE TABLE task_watch_meta(singleton,version,revision); INSERT INTO task_watch_meta VALUES(1,1,0),(2,1,0)",
        "DROP TABLE task_watch_meta; CREATE TABLE task_watch_meta(singleton,version,revision); INSERT INTO task_watch_meta VALUES(1,1,zeroblob(1048576))",
    ] {
        let fixture = Fixture::new("watch-meta-shape");
        let store = fixture.initialize();
        drop(store);
        let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
        raw.execute_batch(schema).unwrap();
        let before: Vec<(Option<i64>, Option<i64>, Option<i64>)> = raw.prepare(
            "SELECT CASE WHEN typeof(singleton)='integer' THEN singleton END, CASE WHEN typeof(version)='integer' THEN version END, CASE WHEN typeof(revision)='integer' THEN revision END FROM task_watch_meta LIMIT 2",
        ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
        drop(raw);
        let result = watch(&fixture.root(), Clock::default())
            .execute(&request(None, 0), deadline())
            .unwrap();
        assert_eq!(result.state, WatchState::Unavailable, "{schema}");
        let raw = fixture.raw();
        let after: Vec<(Option<i64>, Option<i64>, Option<i64>)> = raw.prepare(
            "SELECT CASE WHEN typeof(singleton)='integer' THEN singleton END, CASE WHEN typeof(version)='integer' THEN version END, CASE WHEN typeof(revision)='integer' THEN revision END FROM task_watch_meta LIMIT 2",
        ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(
            after, before,
            "watch observation must preserve malformed source rows"
        );
    }
}

#[test]
fn pending_id_collision_refuses_atomically_without_changing_overlay_or_watch_cursor() {
    let fixture = Fixture::new("pending-collision");
    let store = fixture.initialize();
    publish(&store, &[("already-a-header", b"owned-header")]);
    let retained = TaskId::new("existing-pending".into()).unwrap();
    store
        .replace_pending_fixture(&[retained], deadline())
        .unwrap();
    let raw = fixture.raw();
    let before: (String, i64, Vec<u8>, String) = raw.query_row(
        "SELECT d.generation,w.revision,h.header,(SELECT id FROM task_pending) FROM task_dataset d,task_watch_meta w,task_headers h WHERE d.singleton=1 AND w.singleton=1 AND h.id='already-a-header'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    let colliding = TaskId::new("already-a-header".into()).unwrap();
    assert!(matches!(
        store.replace_pending_fixture(&[colliding], deadline()),
        Err(crate::Error::Incompatible)
    ));
    let after: (String, i64, Vec<u8>, String) = raw.query_row(
        "SELECT d.generation,w.revision,h.header,(SELECT id FROM task_pending) FROM task_dataset d,task_watch_meta w,task_headers h WHERE d.singleton=1 AND w.singleton=1 AND h.id='already-a-header'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(
        after, before,
        "rejected collision must preserve paging generation, metadata revision, header and old overlay"
    );
}

use super::*;
use crate::{Limits, Store};
use rusqlite::Connection as Writer;

#[test]
fn schema_three_root_refuses_without_upgrade_or_repair() {
    let fixture = Fixture::new("capture-schema3");
    let store = fixture.store();
    drop(store);
    let path = fixture.root().join("store.sqlite3");
    let writer = Writer::open(&path).unwrap();
    writer
        .execute_batch(
            "DROP TABLE capture_events; DROP TABLE capture_records; DROP TABLE capture_meta;",
        )
        .unwrap();
    writer.pragma_update(None, "user_version", 3_u32).unwrap();
    drop(writer);
    let marker_path = fixture.root().join("store.identity");
    let current_marker = std::fs::read_to_string(&marker_path).unwrap();
    let generation = current_marker.lines().nth(2).unwrap();
    assert_eq!(current_marker.lines().next(), Some("harflow-storage-2"));
    std::fs::write(
        &marker_path,
        format!("harflow-storage-2\n3\n{generation}\n"),
    )
    .unwrap();
    let marker_before = std::fs::read(&marker_path).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        Store::open(fixture.root(), Limits::default(), deadline()),
        Err(Error::Incompatible)
    ));
    let raw = Writer::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    drop(raw);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "schema refusal changed the stored database"
    );
    assert_eq!(
        std::fs::read(&marker_path).unwrap(),
        marker_before,
        "schema refusal changed the marker"
    );
    let raw = Writer::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let version: u32 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3, "old schema was not upgraded or repaired");
    let tables: i64 = raw.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('capture_records','capture_events','capture_meta')",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        tables, 0,
        "refusal did not add capture schema to the old root"
    );
}

#[test]
fn capture_event_cap_refuses_terminal_without_partial_terminal_event() {
    let fixture = Fixture::new("capture-event-cap");
    let store = fixture.store();
    let binding = accepted_binding(&store, 4);
    store.capture_begin(&binding, deadline()).unwrap();
    for offset in 1..=127 {
        let update = CaptureProgress {
            revision: offset,
            stdout: offset,
            stderr: 0,
            stdout_eof: false,
            stderr_eof: false,
            loss: false,
        };
        store
            .capture_publish(&binding, offset - 1, &update, deadline())
            .unwrap();
    }
    let final_progress = CaptureProgress {
        revision: 128,
        stdout: 128,
        stderr: 0,
        stdout_eof: true,
        stderr_eof: true,
        loss: false,
    };
    store
        .capture_publish(&binding, 127, &final_progress, deadline())
        .unwrap();
    assert!(matches!(
        store.capture_finish(&binding, 128, ProcessExit::Code(0), deadline()),
        Err(Error::LimitExceeded)
    ));
    let connection = raw(&fixture);
    let state: (i64, i64, i64) = connection
        .query_row(
            "SELECT revision,exit_kind,exit_value FROM capture_records WHERE operation=?1",
            [binding.fence.operation.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, (128, 0, 0));
    assert_eq!(event_count(&connection, &binding.fence.operation), 128);
}

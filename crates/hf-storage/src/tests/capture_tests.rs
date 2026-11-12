#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod limits;
mod sequence;
mod support;
use super::super::Error;
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, FileIdentity},
    ownership::Id,
    process::ProcessExit,
};
use rusqlite::{Connection, OpenFlags};
use support::{Fixture, accepted_binding, deadline};

fn raw(fixture: &Fixture) -> Connection {
    Connection::open_with_flags(
        fixture.root().join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

fn event_count(connection: &Connection, operation: &Id) -> i64 {
    connection
        .query_row(
            "SELECT count(*) FROM capture_events WHERE operation=?1",
            [operation.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn begin_has_no_event_and_progress_waits_for_both_streams_before_finish() {
    let fixture = Fixture::new("capture-atomic");
    let store = fixture.store();
    let binding = accepted_binding(&store, 1);
    let begun = store.capture_begin(&binding, deadline()).unwrap();
    assert_eq!(begun.progress.revision, 0);
    assert_eq!(event_count(&raw(&fixture), &binding.fence.operation), 0);

    let first = CaptureProgress {
        revision: 1,
        stdout: 5,
        stderr: 4,
        stdout_eof: false,
        stderr_eof: false,
        loss: false,
    };
    let published = store
        .capture_publish(&binding, 0, &first, deadline())
        .unwrap();
    assert_eq!(published.progress, first);
    let connection = raw(&fixture);
    let row: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT revision,stdout,stderr,loss FROM capture_records WHERE operation=?1",
            [binding.fence.operation.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, (1, 5, 4, 0));
    assert_eq!(event_count(&connection, &binding.fence.operation), 1);
    drop(connection);

    let stdout_eof = CaptureProgress {
        revision: 2,
        stdout: 5,
        stderr: 4,
        stdout_eof: true,
        stderr_eof: false,
        loss: false,
    };
    store
        .capture_publish(&binding, 1, &stdout_eof, deadline())
        .unwrap();
    assert!(matches!(
        store.capture_finish(&binding, 2, ProcessExit::Code(0), deadline()),
        Err(Error::OwnershipConflict)
    ));
    let connection = raw(&fixture);
    assert_eq!(event_count(&connection, &binding.fence.operation), 2);
    let observed = store
        .capture_observe(&binding.fence.operation, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(observed.exit, ProcessExit::Unobserved);
}

#[test]
fn terminal_event_and_capture_row_commit_together_and_are_immutable() {
    let fixture = Fixture::new("capture-terminal");
    let store = fixture.store();
    let binding = accepted_binding(&store, 5);
    store.capture_begin(&binding, deadline()).unwrap();
    let both_eof = CaptureProgress {
        revision: 1,
        stdout: 5,
        stderr: 4,
        stdout_eof: true,
        stderr_eof: true,
        loss: false,
    };
    store
        .capture_publish(&binding, 0, &both_eof, deadline())
        .unwrap();
    let terminal = store
        .capture_finish(&binding, 1, ProcessExit::Code(7), deadline())
        .unwrap();
    assert_eq!(terminal.progress.revision, 2);
    assert_eq!(terminal.exit, ProcessExit::Code(7));
    let connection = raw(&fixture);
    let final_row: (i64, i64, i64, i64, i64, i64) = connection.query_row(
        "SELECT revision,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_records WHERE operation=?1",
        [binding.fence.operation.as_str()],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    ).unwrap();
    assert_eq!(final_row, (2, 1, 1, 0, 1, 7));
    assert_eq!(event_count(&connection, &binding.fence.operation), 2);
    let event_revisions: Vec<i64> = {
        let mut statement = connection
            .prepare("SELECT revision FROM capture_events WHERE operation=?1 ORDER BY revision")
            .unwrap();
        statement
            .query_map([binding.fence.operation.as_str()], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(event_revisions, [1, 2]);
    drop(connection);
    assert!(matches!(
        store.capture_finish(&binding, 2, ProcessExit::Code(7), deadline()),
        Err(Error::OwnershipConflict)
    ));
}

#[test]
fn stale_binding_revision_and_malformed_progress_refuse_without_writing() {
    let fixture = Fixture::new("capture-stale");
    let store = fixture.store();
    let binding = accepted_binding(&store, 2);
    store.capture_begin(&binding, deadline()).unwrap();
    let changed_file = CaptureBinding {
        stdout: FileIdentity {
            device: 1,
            inode: 999,
        },
        ..binding.clone()
    };
    assert!(matches!(
        store.capture_publish(&changed_file, 0, &progress(1, 1, 1), deadline()),
        Err(Error::StaleOwnership)
    ));
    assert!(matches!(
        store.capture_publish(&binding, 1, &progress(2, 1, 1), deadline()),
        Err(Error::StaleOwnership)
    ));
    for malformed in [progress(2, 1, 1), progress(1, 0, 0), progress(1, 65_537, 0)] {
        assert!(matches!(
            store.capture_publish(&binding, 0, &malformed, deadline()),
            Err(Error::InvalidOwnership)
        ));
    }
    let connection = raw(&fixture);
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM capture_records WHERE operation=?1",
            [binding.fence.operation.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision, 0);
    assert_eq!(event_count(&connection, &binding.fence.operation), 0);
}

#[test]
fn known_loss_and_unobserved_or_invalid_exit_never_create_terminal_record() {
    let fixture = Fixture::new("capture-loss");
    let store = fixture.store();
    let binding = accepted_binding(&store, 3);
    store.capture_begin(&binding, deadline()).unwrap();
    assert!(matches!(
        store.capture_finish(&binding, 0, ProcessExit::Unobserved, deadline()),
        Err(Error::InvalidOwnership)
    ));
    assert!(matches!(
        store.capture_finish(&binding, 0, ProcessExit::Signal(0), deadline()),
        Err(Error::InvalidOwnership)
    ));
    let lost = CaptureProgress {
        revision: 1,
        stdout: 65_536,
        stderr: 65_536,
        stdout_eof: true,
        stderr_eof: true,
        loss: true,
    };
    store
        .capture_publish(&binding, 0, &lost, deadline())
        .unwrap();
    assert!(matches!(
        store.capture_finish(&binding, 1, ProcessExit::Code(0), deadline()),
        Err(Error::OwnershipConflict)
    ));
    let observed = store
        .capture_observe(&binding.fence.operation, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(observed.progress.revision, 1);
    assert_eq!(observed.exit, ProcessExit::Unobserved);
}

fn progress(revision: u64, stdout: u64, stderr: u64) -> CaptureProgress {
    CaptureProgress {
        revision,
        stdout,
        stderr,
        stdout_eof: false,
        stderr_eof: false,
        loss: false,
    }
}

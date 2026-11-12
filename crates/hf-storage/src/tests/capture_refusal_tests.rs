#![cfg(unix)]
#![allow(clippy::duplicate_mod)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "capture_tests/support.rs"]
mod support;

use super::super::{Error, Limits, Store};
use hf_domain::capture::CaptureProgress;
use rusqlite::{Connection, OpenFlags};
use support::{Fixture, accepted_binding, deadline};

#[test]
fn low_byte_limit_refuses_capture_begin_before_creating_rows() {
    let fixture = Fixture::new("begin-limit");
    let store = fixture.store();
    let binding = accepted_binding(&store, 51);
    drop(store);

    let limited = Store::open(
        fixture.root(),
        Limits {
            max_rows: 128,
            max_bytes: 1,
        },
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    assert!(matches!(
        limited.capture_begin(&binding, deadline()),
        Err(Error::LimitExceeded)
    ));
    drop(limited);

    let raw = readonly(&fixture);
    assert_eq!(count(&raw, "capture_records"), 0);
    assert_eq!(count(&raw, "capture_events"), 0);
}

#[test]
fn low_byte_limit_refuses_progress_without_advancing_snapshot_or_event() {
    let fixture = Fixture::new("publish-limit");
    let store = fixture.store();
    let binding = accepted_binding(&store, 52);
    store.capture_begin(&binding, deadline()).unwrap();
    drop(store);

    let limited = Store::open(
        fixture.root(),
        Limits {
            max_rows: 128,
            max_bytes: 1,
        },
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let progress = CaptureProgress {
        revision: 1,
        stdout: 1,
        stderr: 1,
        stdout_eof: false,
        stderr_eof: false,
        loss: false,
    };
    assert!(matches!(
        limited.capture_publish(&binding, 0, &progress, deadline()),
        Err(Error::LimitExceeded)
    ));
    drop(limited);

    let raw = readonly(&fixture);
    let row: (i64, i64, i64) = raw
        .query_row(
            "SELECT revision,stdout,stderr FROM capture_records WHERE operation=?1",
            [binding.fence.operation.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, (0, 0, 0));
    assert_eq!(count(&raw, "capture_events"), 0);
}

#[test]
fn malformed_metadata_and_capture_scalars_refuse_without_repair() {
    for corruption in ["version_type", "oversized_binding", "negative_revision"] {
        assert_malformed_row_is_preserved(corruption);
    }
}

fn assert_malformed_row_is_preserved(corruption: &str) {
    let fixture = Fixture::new(corruption);
    let store = fixture.store();
    let binding = accepted_binding(&store, 53);
    store.capture_begin(&binding, deadline()).unwrap();
    drop(store);

    let raw = crate::tests::raw_connection(fixture.root().join("store.sqlite3"));
    match corruption {
        "version_type" => raw
            .execute(
                "UPDATE capture_meta SET version='bad' WHERE singleton=1",
                [],
            )
            .unwrap(),
        "oversized_binding" => raw
            .execute("UPDATE capture_records SET binding=printf('%0513d',0)", [])
            .unwrap(),
        "negative_revision" => raw
            .execute("UPDATE capture_records SET revision=-1", [])
            .unwrap(),
        _ => panic!("unknown corruption case"),
    };
    drop(raw);

    let reopened = Store::open(
        fixture.root(),
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    assert!(
        matches!(
            reopened.capture_observe(&binding.fence.operation, deadline()),
            Err(Error::InvalidOwnership)
        ),
        "{corruption}"
    );
    drop(reopened);

    let raw = readonly(&fixture);
    match corruption {
        "version_type" => {
            let value: (String, String) = raw.query_row(
                "SELECT typeof(version),CAST(version AS TEXT) FROM capture_meta WHERE singleton=1",
                [], |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap();
            assert_eq!(value, ("text".into(), "bad".into()));
        }
        "oversized_binding" => {
            let length: i64 = raw
                .query_row(
                    "SELECT length(CAST(binding AS BLOB)) FROM capture_records",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(length, 513);
        }
        "negative_revision" => {
            let revision: i64 = raw
                .query_row("SELECT revision FROM capture_records", [], |row| row.get(0))
                .unwrap();
            assert_eq!(revision, -1);
        }
        _ => unreachable!(),
    }
}

fn readonly(fixture: &Fixture) -> Connection {
    Connection::open_with_flags(
        fixture.root().join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}
fn count(raw: &Connection, table: &str) -> i64 {
    raw.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

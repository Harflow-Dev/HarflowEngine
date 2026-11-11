#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::{
    event_read::{EventRecord, RecordClass},
    task_read::TaskRow,
};
use hf_domain::{
    event_read::{CaptureState, EventFilter, EventQuery, LogFacts},
    task_read::{Coverage, StoredObservation, TaskId},
};
use hf_storage::{EventFixture, Store, TaskFixture};

const WINDOW: u64 = 1_048_576;
fn task() -> TaskId {
    TaskId::new("resync-task-1234".into()).unwrap()
}
fn setup() -> (Fixture, Store) {
    let dir = Fixture::new();
    let store = Store::initialize_fixture(
        dir.path().join("store"),
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![TaskRow {
                    id: task(),
                    stamp: 1,
                    session: None,
                    observation: StoredObservation {
                        recorded_status: Some("running".into()),
                        active: Some(true),
                        exited: None,
                        drained: Some(false),
                        owned: Some(true),
                    },
                    header: b"row".to_vec(),
                    status: None,
                }],
                launches: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline(),
        )
        .unwrap();
    (dir, store)
}
fn event(start: u64) -> EventRecord {
    let body = br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":7,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"after gap"}"#.to_vec();
    EventRecord {
        start,
        end: start + body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(7),
        kind: Some("assistant_text".into()),
        body,
    }
}
fn query(oldest: bool, cursor: Option<hf_domain::event_read::EventCursor>) -> EventQuery {
    EventQuery {
        task: task(),
        limit: 20,
        filter: EventFilter::All,
        from_oldest: oldest,
        cursor,
    }
}

#[test]
fn oversized_mult_window_resync_progresses_forward_and_backward_to_short_event() {
    let (_dir, store) = setup();
    let gap_end = 3 * WINDOW + 17;
    let short = event(gap_end);
    let end = short.end;
    let facts = LogFacts {
        available: true,
        capture: CaptureState::Live,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
        task_status: Some("running".into()),
    };
    store
        .replace_event_fixture(
            &EventFixture {
                task: task(),
                records: vec![
                    EventRecord {
                        start: 0,
                        end: gap_end,
                        class: RecordClass::Oversized,
                        seq: None,
                        kind: None,
                        body: Vec::new(),
                    },
                    short,
                ],
                partial_tail: 37,
                facts,
            },
            deadline(),
        )
        .unwrap();

    let mut forward = store.event_read(&query(true, None), deadline()).unwrap();
    let mut previous_boundary = 0;
    let mut found_forward = false;
    for _ in 0..6 {
        assert!(forward.scanned_bytes <= WINDOW);
        if forward.events.iter().any(|item| item.seq == Some(7)) {
            found_forward = true;
            break;
        }
        let cursor = forward.newer_cursor.clone().unwrap();
        assert!(cursor.boundary > previous_boundary);
        assert!(cursor.boundary - previous_boundary <= WINDOW);
        previous_boundary = cursor.boundary;
        assert!(forward.has_newer);
        forward = store
            .event_read(&query(false, Some(cursor)), deadline())
            .unwrap();
    }
    assert!(
        found_forward,
        "forward resync did not reach the event within six bounded windows"
    );

    let mut backward = store.event_read(&query(false, None), deadline()).unwrap();
    assert!(backward.events.iter().any(|item| item.seq == Some(7)));
    let mut previous_boundary = end + 37;
    let mut reached_start = false;
    for _ in 0..6 {
        assert!(backward.scanned_bytes <= WINDOW);
        let Some(cursor) = backward.older_cursor.clone() else {
            reached_start = true;
            break;
        };
        assert!(cursor.boundary < previous_boundary);
        assert!(previous_boundary - cursor.boundary <= WINDOW);
        previous_boundary = cursor.boundary;
        backward = store
            .event_read(&query(false, Some(cursor)), deadline())
            .unwrap();
    }
    assert!(
        reached_start || backward.older_cursor.is_none(),
        "backward resync failed to finish within six bounded windows"
    );
}

#[test]
fn sql_oversize_body_is_rejected_before_projection() {
    let (_dir, store) = setup();
    let record = event(0);
    store
        .replace_event_fixture(
            &EventFixture {
                task: task(),
                records: vec![record],
                partial_tail: 0,
                facts: LogFacts {
                    available: true,
                    capture: CaptureState::Live,
                    output_drained: None,
                    eof_finished_seq: None,
                    within_grace: false,
                    task_status: Some("running".into()),
                },
            },
            deadline(),
        )
        .unwrap();
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    db.execute("UPDATE event_records SET body=zeroblob(20000)", [])
        .unwrap();
    drop(db);
    assert!(matches!(
        store.event_read(&query(false, None), deadline()),
        Err(hf_storage::Error::LimitExceeded)
    ));
}

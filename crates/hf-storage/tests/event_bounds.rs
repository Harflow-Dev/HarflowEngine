#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_application::task_read::TaskRow;
use hf_domain::{
    event_read::{CaptureState, EventFilter, EventQuery, LogFacts},
    task_read::{Coverage, StoredObservation, TaskId},
};
use hf_storage::{EventFixture, Store, TaskFixture};

fn task() -> TaskId {
    TaskId::new("bounds-task-1234".into()).unwrap()
}
fn setup() -> (Fixture, Store) {
    let dir = Fixture::new();
    let store =
        Store::initialize_fixture(dir.path().join("store"), default_limits(), deadline()).unwrap();
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
                pending: Vec::new(),
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
fn facts() -> LogFacts {
    LogFacts {
        available: true,
        capture: CaptureState::Live,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
        task_status: Some("running".into()),
    }
}
fn query(oldest: bool, cursor: Option<hf_domain::event_read::EventCursor>) -> EventQuery {
    EventQuery {
        task: task(),
        limit: 200,
        filter: EventFilter::All,
        from_oldest: oldest,
        cursor,
    }
}
fn event(start: u64, seq: u64, body: Vec<u8>) -> EventRecord {
    let end = start + body.len() as u64 + 1;
    EventRecord {
        start,
        end,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some("assistant_text".into()),
        body,
    }
}
fn fixture(records: Vec<EventRecord>, partial_tail: u64) -> EventFixture {
    EventFixture {
        task: task(),
        records,
        partial_tail,
        facts: facts(),
    }
}

#[test]
fn page_budget_trims_without_overlapping_and_keeps_far_side_reachable() {
    let (_dir, store) = setup();
    let mut records = Vec::new();
    let mut pos = 0;
    for seq in 0..40 {
        let body = format!(r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"{}"}}"#, "x".repeat(9_000)).into_bytes();
        let row = event(pos, seq, body);
        pos = row.end;
        records.push(row);
    }
    store
        .replace_event_fixture(&fixture(records, 0), deadline())
        .unwrap();
    let mut page = store.event_read(&query(true, None), deadline()).unwrap();
    assert!(page.events.len() < 40);
    assert!(page.has_newer);
    let mut seen = Vec::new();
    for _ in 0..16 {
        seen.extend(page.events.iter().map(|event| event.seq.unwrap()));
        let Some(cursor) = page.newer_cursor else {
            break;
        };
        let next = store
            .event_read(&query(false, Some(cursor)), deadline())
            .unwrap();
        if next.events.is_empty() {
            break;
        }
        assert!(page.span.unwrap().1 <= next.span.unwrap().0);
        page = next;
    }
    assert_eq!(seen, (0..40).collect::<Vec<_>>());
    let newest = store.event_read(&query(false, None), deadline()).unwrap();
    assert!(newest.events.len() < 40);
    let mut reverse_pages = vec![
        newest
            .events
            .iter()
            .map(|event| event.seq.unwrap())
            .collect::<Vec<_>>(),
    ];
    let mut reverse_page = newest;
    for _ in 0..16 {
        let Some(cursor) = reverse_page.older_cursor.clone() else {
            break;
        };
        let previous = store
            .event_read(&query(false, Some(cursor)), deadline())
            .unwrap();
        if previous.events.is_empty() {
            break;
        }
        assert!(previous.span.unwrap().1 <= reverse_page.span.unwrap().0);
        assert!(
            previous.events.last().unwrap().seq.unwrap()
                < reverse_page.events.first().unwrap().seq.unwrap()
        );
        let prior = previous
            .events
            .iter()
            .map(|event| event.seq.unwrap())
            .collect::<Vec<_>>();
        reverse_pages.push(prior);
        reverse_page = previous;
    }
    reverse_pages.reverse();
    let reverse = reverse_pages.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(reverse, (0..40).collect::<Vec<_>>());
}

#[test]
fn window_clips_crossing_record_and_partial_tail_is_not_newer_content() {
    let (_dir, store) = setup();
    let body = br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":1,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"tail"}"#.to_vec();
    let end = 1_000_001;
    let record = EventRecord {
        start: 0,
        end,
        class: RecordClass::Valid,
        seq: Some(1),
        kind: Some("assistant_text".into()),
        body,
    };
    store
        .replace_event_fixture(&fixture(vec![record], 100_000), deadline())
        .unwrap();
    let newest = store.event_read(&query(false, None), deadline()).unwrap();
    assert_eq!(newest.scanned_bytes, 1_048_576);
    assert!(newest.events.is_empty());
    assert_eq!(newest.decoded, 1);
    assert_eq!(newest.span, None);
    assert_eq!(newest.older_cursor.as_ref().unwrap().boundary, end);
    assert!(!newest.has_newer);
    let older = store
        .event_read(&query(false, newest.older_cursor), deadline())
        .unwrap();
    assert_eq!(
        older
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        [Some(1)]
    );
    assert_eq!(older.oversized, 0);
    assert_eq!(older.malformed, 0);
}

#[test]
fn eof_final_record_is_detected_independently_of_capture_and_gap() {
    let (_dir, store) = setup();
    let first = event(0, 1, br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":1,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"a"}"#.to_vec());
    let gap = EventRecord {
        start: first.end,
        end: first.end + 6,
        class: RecordClass::Malformed,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    let final_body = br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":3,"kind":"task_finished","observed_at":"2026-01-01T00:00:00Z","text":"done"}"#.to_vec();
    let final_record = EventRecord {
        start: gap.end,
        end: gap.end + final_body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(3),
        kind: Some("task_finished".into()),
        body: final_body,
    };
    let mut f = fixture(vec![first, gap, final_record], 0);
    f.facts.capture = CaptureState::Unknown;
    store.replace_event_fixture(&f, deadline()).unwrap();
    let result = store.event_read(&query(false, None), deadline()).unwrap();
    let unknown = result.facts.unwrap();
    assert_eq!(unknown.eof_finished_seq, Some(3));
    assert_eq!(unknown.state(), hf_domain::event_read::LogState::Unknown);
    f.facts.capture = CaptureState::ObservedTerminal;
    f.facts.output_drained = Some(true);
    store.replace_event_fixture(&f, deadline()).unwrap();
    let terminal_result = store.event_read(&query(false, None), deadline()).unwrap();
    let final_facts = terminal_result.facts.unwrap();
    assert_eq!(final_facts.eof_finished_seq, Some(3));
    assert_eq!(final_facts.state(), hf_domain::event_read::LogState::Final);
    assert_eq!(terminal_result.gaps.len(), 1);
    assert_eq!(terminal_result.gaps[0].after_seq, 1);
    assert_eq!(terminal_result.gaps[0].before_seq, 3);
    assert_eq!(
        terminal_result.gaps[0].reason,
        hf_domain::event_read::GapReason::UnreadableRecords
    );
    store.replace_event_fixture(&fixture(vec![EventRecord { start: 0, end: final_record_end(), class: RecordClass::Valid, seq: Some(3), kind: Some("task_finished".into()), body: br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":3,"kind":"task_finished","observed_at":"2026-01-01T00:00:00Z","text":"done"}"#.to_vec() }], 1), deadline()).unwrap();
    let partial = store.event_read(&query(false, None), deadline()).unwrap();
    assert_eq!(partial.facts.unwrap().eof_finished_seq, None);
}

fn final_record_end() -> u64 {
    br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":3,"kind":"task_finished","observed_at":"2026-01-01T00:00:00Z","text":"done"}"#.len() as u64 + 1
}

#[test]
fn future_dataset_version_fails_closed_without_mutating_rows() {
    let (_dir, store) = setup();
    store.replace_event_fixture(&fixture(vec![event(0, 1, br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":1,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"x"}"#.to_vec())], 0), deadline()).unwrap();
    let before: (i64, i64, String, Vec<u8>) = {
        let path = _dir.path().join("store/store.sqlite3");
        let db = rusqlite::Connection::open(path).unwrap();
        db.query_row(
            "SELECT (SELECT version FROM event_dataset),(SELECT count(*) FROM event_records),(SELECT generation FROM event_streams),(SELECT body FROM event_records)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    };
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    db.execute("UPDATE event_dataset SET version=99", [])
        .unwrap();
    drop(db);
    assert!(store.event_read(&query(false, None), deadline()).is_err());
    assert!(
        store
            .replace_event_fixture(&fixture(Vec::new(), 0), deadline())
            .is_err()
    );
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    let after: (i64, i64, String, Vec<u8>) = db
        .query_row(
            "SELECT (SELECT version FROM event_dataset),(SELECT count(*) FROM event_records),(SELECT generation FROM event_streams),(SELECT body FROM event_records)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(after, (99, before.1, before.2, before.3));
    assert!(matches!(
        store.event_read(&query(false, None), deadline()),
        Err(hf_storage::Error::EventReadUnavailable)
    ));
}

#[test]
fn malformed_metadata_and_sql_oversize_body_fail_closed_at_projection() {
    let (_dir, store) = setup();
    let body = br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":1,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"x"}"#.to_vec();
    store
        .replace_event_fixture(
            &fixture(
                vec![EventRecord {
                    start: 0,
                    end: body.len() as u64 + 1,
                    class: RecordClass::Valid,
                    seq: Some(1),
                    kind: Some("assistant_text".into()),
                    body,
                }],
                0,
            ),
            deadline(),
        )
        .unwrap();
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    db.execute("UPDATE event_streams SET generation='bad'", [])
        .unwrap();
    drop(db);
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    let before: (String, Vec<u8>) = db
        .query_row(
            "SELECT generation,(SELECT body FROM event_records) FROM event_streams",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    drop(db);
    assert!(matches!(
        store.event_read(&query(false, None), deadline()),
        Err(hf_storage::Error::EventReadUnavailable)
    ));
    assert!(
        store
            .replace_event_fixture(&fixture(Vec::new(), 0), deadline())
            .is_err()
    );
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    let after: (String, Vec<u8>) = db
        .query_row(
            "SELECT generation,(SELECT body FROM event_records) FROM event_streams",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn expired_append_and_read_leave_existing_event_rows_unchanged() {
    let (_dir, store) = setup();
    let body = br#"{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":1,"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"kept"}"#.to_vec();
    let record = EventRecord {
        start: 0,
        end: body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(1),
        kind: Some("assistant_text".into()),
        body,
    };
    store
        .replace_event_fixture(&fixture(vec![record.clone()], 0), deadline())
        .unwrap();
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    let before: (String, i64, Vec<u8>) = db
        .query_row(
            "SELECT generation,complete_end,(SELECT body FROM event_records) FROM event_streams",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    drop(db);
    let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert!(matches!(
        store.append_event_fixture(&fixture(Vec::new(), 1), expired),
        Err(hf_storage::Error::DeadlineExceeded)
    ));
    assert!(matches!(
        store.event_read(&query(false, None), expired),
        Err(hf_storage::Error::DeadlineExceeded)
    ));
    let db = rusqlite::Connection::open(_dir.path().join("store/store.sqlite3")).unwrap();
    let after: (String, i64, Vec<u8>) = db
        .query_row(
            "SELECT generation,complete_end,(SELECT body FROM event_records) FROM event_streams",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

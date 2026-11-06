#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Event, id and stored-status reads on a read-only connection over a WAL-only store,
//! compared with values generated here and with the writer's own answers.
mod common;
mod read_session_support;
use hf_application::{
    event_read::{EventRecord, RecordClass},
    task_read::TaskRow,
};
use hf_domain::{
    event_read::{CaptureState, EventFilter, EventQuery, LogFacts},
    task_read::{AuthorizedScope, Coverage, Lookup, StoredObservation, TaskId, TaskQuery},
};
use hf_storage::{EventFixture, ReadSession, Store, TaskFixture};
use read_session_support::{later, row, rows};

fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

fn event_task_row() -> TaskRow {
    TaskRow {
        id: id("event-task-1234"),
        stamp: 1,
        session: None,
        observation: StoredObservation {
            recorded_status: Some("running".into()),
            active: Some(true),
            exited: None,
            drained: Some(false),
            owned: Some(true),
        },
        header: b"fixture".to_vec(),
        status: None,
    }
}

fn facts(available: bool) -> LogFacts {
    LogFacts {
        available,
        capture: CaptureState::Live,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
        task_status: Some("running".into()),
    }
}

fn event_record(start: u64, seq: u64, kind: &str) -> EventRecord {
    let raw = format!(r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"{kind}","observed_at":"2026-01-01T00:00:00Z","text":"record-{seq}"}}"#).into_bytes();
    EventRecord {
        start,
        end: start + raw.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some(kind.into()),
        body: raw,
    }
}

fn query(
    limit: usize,
    from_oldest: bool,
    filter: EventFilter,
    cursor: Option<hf_domain::event_read::EventCursor>,
) -> EventQuery {
    EventQuery {
        task: id("event-task-1234"),
        limit,
        filter,
        from_oldest,
        cursor,
    }
}

const KINDS: [&str; 6] = [
    "task_started",
    "assistant_text",
    "tool_call",
    "tool_result",
    "usage",
    "task_finished",
];

fn records() -> Vec<EventRecord> {
    let mut start = 0;
    KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            let record = event_record(start, index as u64 + 1, kind);
            start = record.end;
            record
        })
        .collect()
}

struct Built {
    _scratch: common::Fixture,
    writer: Store,
    session: ReadSession,
    wal: std::path::PathBuf,
    all_rows: Vec<hf_application::task_read::TaskRow>,
}

fn built() -> Built {
    let scratch = common::Fixture::new();
    let root = scratch.path().join("store");
    let writer = Store::initialize_fixture(
        &root,
        common::default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    let mut all_rows = rows(1, 6);
    all_rows.push(event_task_row());
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: all_rows.clone(),
                pending: vec![id("pending-task-9999")],
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            later(5),
        )
        .unwrap();
    writer
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: records(),
                partial_tail: 0,
                facts: facts(true),
            },
            later(5),
        )
        .unwrap();
    let session = ReadSession::open(
        &root,
        common::default_limits(),
        later(5),
        common::lease::lease_port(),
    )
    .unwrap();
    Built {
        wal: root.join("store.sqlite3-wal"),
        _scratch: scratch,
        writer,
        session,
        all_rows,
    }
}

#[test]
fn event_pages_continue_by_cursor_with_the_exact_stored_records() {
    let built = built();
    assert!(
        std::fs::metadata(&built.wal).unwrap().len() > 0,
        "data must be in the WAL"
    );
    let want = records();
    let mut seen = Vec::new();
    let mut result = built
        .session
        .event_read(&query(2, true, EventFilter::All, None), later(5))
        .unwrap();
    let mut pages = 0;
    loop {
        pages += 1;
        assert_eq!(result.lookup, Lookup::Found(id("event-task-1234")));
        assert!(result.events.len() <= 2);
        seen.extend(result.events.clone());
        let Some(cursor) = result.newer_cursor.clone().filter(|_| result.has_newer) else {
            break;
        };
        result = built
            .session
            .event_read(&query(2, false, EventFilter::All, Some(cursor)), later(5))
            .unwrap();
        assert!(pages < 10, "the cursor did not advance");
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, want);
    assert_eq!(result.span, Some((5, 6)));
    assert!(!result.has_newer);
}

#[test]
fn the_newest_page_and_the_whole_answer_equal_the_writers_answer() {
    let built = built();
    for (limit, oldest) in [(3, false), (3, true), (10, false)] {
        let q = query(limit, oldest, EventFilter::All, None);
        let from_session = built.session.event_read(&q, later(5)).unwrap();
        let from_writer = built.writer.event_read(&q, later(5)).unwrap();
        assert_eq!(from_session, from_writer, "limit {limit} oldest {oldest}");
    }
    let newest = built
        .session
        .event_read(&query(3, false, EventFilter::All, None), later(5))
        .unwrap();
    assert_eq!(newest.events, records()[3..].to_vec());
    assert!(newest.has_older);
}

#[test]
fn ids_reads_return_found_pending_and_missing_with_the_stored_rows() {
    let built = built();
    let wanted = [
        row(1, 3).id,
        id("pending-task-9999"),
        row(1, 1).id,
        id("absent-task-0000"),
    ];
    let q = TaskQuery::ids(wanted.to_vec()).unwrap();
    let result = built
        .session
        .task_read(&AuthorizedScope::Global, &q, later(5))
        .unwrap();
    assert_eq!(
        result.lookups,
        vec![
            Lookup::Found(row(1, 3).id),
            Lookup::Pending(id("pending-task-9999")),
            Lookup::Found(row(1, 1).id),
            Lookup::Missing,
        ]
    );
    let mut got = result.rows.clone();
    got.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    assert_eq!(got, vec![row(1, 1), row(1, 3)]);
    let from_writer = built
        .writer
        .task_read(&AuthorizedScope::Global, &q, later(5))
        .unwrap();
    assert_eq!(result, from_writer);
}

#[test]
fn stored_status_returns_the_exact_stored_row_or_a_missing_lookup() {
    let built = built();
    for number in [0, 2, 5] {
        let target = row(1, number);
        let q = TaskQuery::StoredStatus(target.id.clone());
        let result = built
            .session
            .task_read(&AuthorizedScope::Global, &q, later(5))
            .unwrap();
        assert_eq!(result.rows, vec![target.clone()], "task {number}");
        assert_eq!(result.rows[0].status, target.status);
        assert_eq!(result.rows[0].header, target.header);
    }
    let q = TaskQuery::StoredStatus(id("absent-task-0000"));
    let result = built
        .session
        .task_read(&AuthorizedScope::Global, &q, later(5))
        .unwrap();
    assert!(result.rows.is_empty());
    assert_eq!(built.all_rows.len(), 7);
}

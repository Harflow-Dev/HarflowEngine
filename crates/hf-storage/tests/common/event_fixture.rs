#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "mod.rs"]
mod common;
pub(super) use common::{Fixture, deadline, default_limits};
pub(super) use hf_application::{
    event_read::{EventRecord, RecordClass},
    task_read::TaskRow,
};
pub(super) use hf_domain::{
    event_read::{CaptureState, EventFilter, EventQuery, LogFacts},
    task_read::{Coverage, Lookup, StoredObservation, TaskId},
};
pub(super) use hf_storage::{EventFixture, Store, TaskFixture};

pub(super) fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}
pub(super) fn row() -> TaskRow {
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
pub(super) fn store() -> (Fixture, Store) {
    let fixture = Fixture::new();
    let store =
        Store::initialize_fixture(fixture.path().join("store"), default_limits(), deadline())
            .unwrap();
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row()],
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
    (fixture, store)
}
pub(super) fn facts(available: bool) -> LogFacts {
    LogFacts {
        available,
        capture: CaptureState::Live,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
        task_status: Some("running".into()),
    }
}
pub(super) fn event_record(start: u64, seq: u64, kind: &str) -> EventRecord {
    let raw = format!(r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"{kind}","observed_at":"2026-01-01T00:00:00Z","text":"record-{seq}"}}"#).into_bytes();
    let end = start + raw.len() as u64 + 1;
    EventRecord {
        start,
        end,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some(kind.into()),
        body: raw,
    }
}
pub(super) fn query(
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

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Roots and reads for the feed tests. The reads behind recovery-activity, interaction-events and statistics-page against a real
//! root: keyset order, measured bytes, oversized and over-budget rows, and the generation.
use crate::common::{Fixture, deadline, default_limits};
use hf_application::ctl::task_reads::ScanRow;
use hf_domain::{
    ctl::statistics::StatFacts,
    task_read::{Coverage, TaskId},
};
use hf_storage::{
    FixtureFacts, FixtureLine, FixturePrompt, FixtureTask, Limits, ReadSession, Store, TaskFixture,
};
use std::path::PathBuf;

pub fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

pub fn task(name: &str) -> FixtureTask {
    FixtureTask {
        facts: FixtureFacts {
            id: id(name),
            backend: "claude".into(),
            repo_path: "/repo".into(),
            session_id: "s".into(),
            status: "completed".into(),
            exit_code: Some(0),
            started_at: "2026-01-10T10:00:00+00:00".into(),
            started_at_us: 1_768_039_200_000_000,
            finished_at: Some("2026-01-10T10:05:00+00:00".into()),
            finished_at_us: Some(1_768_039_500_000_000),
            output_drained: true,
            title: Some(format!("title {name}")),
            model: None,
            continuation_of: None,
            chain_root: None,
        },
        header: format!("{{\"task_id\":\"{name}\"}}").into_bytes(),
        status: b"{}".to_vec(),
        prompt: FixturePrompt {
            inline_text: Some("p".into()),
            preview: None,
            claimed_sha256: None,
            source_name: None,
            blob: None,
        },
        process: None,
    }
}

pub struct Root {
    pub _scratch: Fixture,
    pub path: PathBuf,
}

pub fn root(tasks: Vec<FixtureTask>) -> Root {
    root_with(tasks, false)
}

/// With `stream`, the first task also has an event stream.
pub fn root_with(tasks: Vec<FixtureTask>, stream: bool) -> Root {
    let scratch = Fixture::new();
    let path = scratch.path().join("store");
    let store = Store::initialize_fixture(
        &path,
        default_limits(),
        deadline(),
        crate::common::lease::lease_port(),
    )
    .unwrap();
    let all = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: Vec::new(),
                launches: Vec::new(),
                coverage: all,
            },
            deadline(),
        )
        .unwrap();
    for one in &tasks {
        store.add_task_fixture(one, deadline()).unwrap();
    }
    if stream {
        let line = FixtureLine::Event {
            seq: 0,
            kind: "notice".into(),
            body: b"{}".to_vec(),
        };
        store
            .append_event_record_fixture(&tasks[0].facts.id, &line, deadline())
            .unwrap();
    }
    drop(store);
    Root {
        _scratch: scratch,
        path,
    }
}

pub fn read<T>(
    root: &Root,
    limits: Limits,
    action: impl FnOnce(&hf_storage::TaskSnapshot<'_>) -> hf_storage::Result<T>,
) -> hf_storage::Result<T> {
    let session = ReadSession::open(
        &root.path,
        limits,
        deadline(),
        crate::common::lease::lease_port(),
    )?;
    let mut action = Some(action);
    let mut out = None;
    session.snapshot(deadline(), |snapshot| {
        out = Some(action.take().unwrap()(snapshot)?);
        Ok(())
    })?;
    Ok(out.unwrap())
}

/// The limits the host opens its read sessions with.
pub fn host() -> Limits {
    Limits {
        max_rows: 201,
        max_bytes: 1_048_576,
    }
}

pub fn names(count: usize) -> Vec<FixtureTask> {
    (0..count).map(|n| task(&format!("t{n:04}"))).collect()
}

pub fn facts_of(row: &ScanRow) -> &StatFacts {
    match row {
        ScanRow::Facts(facts) => facts,
        other => panic!("{other:?}"),
    }
}

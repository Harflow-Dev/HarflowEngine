#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{Limits, Store, TaskFixture};
use hf_application::task_watch::{TaskWatchPort, WatchCodecPort, WatchControl, WatchError};
use hf_domain::{
    task_read::{Coverage, StoredObservation, TaskId},
    task_watch::{WatchCursor, WatchRequest},
};
mod support;
use self::support::{Actor, Fixture};
use rusqlite::Connection;
use std::{
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(5);

#[test]
fn actor_child() {
    let Ok(root) = std::env::var("HF_WATCH_ATOMIC_ROOT") else {
        return;
    };
    let ledger = PathBuf::from(std::env::var("HF_WATCH_ATOMIC_LEDGER").unwrap());
    if let Ok(stage) = std::env::var("HF_WATCH_ATOMIC_STAGE") {
        *crate::tests::STAGE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(stage);
    }
    let store = Store::open(
        root,
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    println!("READY");
    std::io::stdout().flush().unwrap();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().take(4) {
        let command = line.unwrap();
        if command == "STOP" {
            return;
        }
        let header = match command.as_str() {
            "PUBLISH_A" => b"writer-a".as_slice(),
            "PUBLISH_B" => b"writer-b".as_slice(),
            _ => panic!("unexpected fixed actor command"),
        };
        let result = store.replace_task_fixture(&fixture(header), deadline());
        if result.is_ok() {
            append_ledger(&ledger, header);
        }
        println!(
            "RESULT:{}",
            if result.is_ok() { "COMMITTED" } else { "BUSY" }
        );
        std::io::stdout().flush().unwrap();
    }
}

#[test]
fn independent_live_publishers_observe_busy_then_explicit_commit_with_atomic_rows_and_revision() {
    let fixture = Fixture::new();
    let initialized = Store::initialize_fixture(
        fixture.root(),
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    drop(initialized);
    let baseline = watch(&fixture.root())
        .execute(
            &WatchRequest {
                cursor: None,
                wait_seconds: 0,
            },
            deadline(),
        )
        .unwrap();
    assert!(baseline.cursor.is_some());
    assert_eq!(
        revision(&fixture.raw()),
        0,
        "unpaired observer performs no writer transition"
    );
    let mut first = Actor::spawn(
        &fixture.root(),
        Some("before_task_catalog_commit"),
        &fixture.ledger(),
    );
    let mut second = Actor::spawn(&fixture.root(), None, &fixture.ledger());
    first.send("PUBLISH_A");
    first.expect("STAGE:before_task_catalog_commit");
    let raw = fixture.raw();
    assert_eq!(revision(&raw), 0);
    assert!(header(&raw).is_none());
    drop(raw);

    // A fresh observer is not a publisher right: root qualification is busy while A owns I/O.
    let observer = watch(&fixture.root()).execute(
        &WatchRequest {
            cursor: None,
            wait_seconds: 0,
        },
        deadline(),
    );
    assert!(matches!(
        observer,
        Err(WatchError::StateBusy) | Err(WatchError::DeadlineExceeded)
    ));
    second.send("PUBLISH_B");
    second.expect("RESULT:BUSY");
    let raw = fixture.raw();
    assert_eq!(revision(&raw), 0);
    assert!(header(&raw).is_none());
    drop(raw);
    assert!(!fixture.ledger().exists());

    first.terminate();
    second.send("PUBLISH_B");
    second.expect("RESULT:COMMITTED");
    let raw = fixture.raw();
    assert_eq!(revision(&raw), 1);
    assert_eq!(header(&raw).as_deref(), Some(b"writer-b".as_slice()));
    drop(raw);
    assert_eq!(fs::read(fixture.ledger()).unwrap(), b"writer-b\n");
    second.send("STOP");
    second.terminate();
}

#[test]
fn committed_rows_survive_lost_ack_and_remaining_live_writer_commits_next_revision() {
    let fixture = Fixture::new();
    let initialized = Store::initialize_fixture(
        fixture.root(),
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    drop(initialized);
    let mut first = Actor::spawn(
        &fixture.root(),
        Some("after_task_catalog_commit"),
        &fixture.ledger(),
    );
    let mut second = Actor::spawn(&fixture.root(), None, &fixture.ledger());
    first.send("PUBLISH_A");
    first.expect("STAGE:after_task_catalog_commit");
    let raw = fixture.raw();
    assert_eq!(revision(&raw), 1);
    assert_eq!(header(&raw).as_deref(), Some(b"writer-a".as_slice()));
    drop(raw);
    assert!(
        !fixture.ledger().exists(),
        "external ledger follows successful return only"
    );
    second.send("PUBLISH_B");
    second.expect("RESULT:BUSY");
    first.terminate();
    second.send("PUBLISH_B");
    second.expect("RESULT:COMMITTED");
    let raw = fixture.raw();
    assert_eq!(revision(&raw), 2);
    assert_eq!(header(&raw).as_deref(), Some(b"writer-b".as_slice()));
    drop(raw);
    assert_eq!(fs::read(fixture.ledger()).unwrap(), b"writer-b\n");
    second.send("STOP");
    second.terminate();
}

#[derive(Clone)]
struct Codec;
impl WatchCodecPort for Codec {
    fn decode(&self, raw: &str, root: &str) -> Result<WatchCursor, WatchError> {
        let parts: Vec<_> = raw.split(':').collect();
        if parts.len() != 3 || parts[0] != root {
            return Err(WatchError::InvalidCursor);
        }
        Ok(WatchCursor {
            root: parts[0].into(),
            epoch: parts[1].into(),
            revision: parts[2].parse().map_err(|_| WatchError::InvalidCursor)?,
        })
    }
    fn encode(&self, value: &WatchCursor) -> Result<String, WatchError> {
        Ok(format!("{}:{}:{}", value.root, value.epoch, value.revision))
    }
}
struct Immediate;
impl WatchControl for Immediate {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn check(&self) -> Result<(), WatchError> {
        Ok(())
    }
    fn wait_until(&self, _: Instant) -> Result<(), WatchError> {
        Err(WatchError::Cancelled)
    }
}
fn watch(root: &Path) -> crate::WatchSession<Codec, Immediate> {
    crate::WatchSession::new(
        root,
        Limits::default(),
        Codec,
        Immediate,
        crate::tests::lease::lease_port(),
    )
}
fn fixture(header: &[u8]) -> TaskFixture {
    TaskFixture {
        rows: vec![hf_application::task_read::TaskRow {
            id: TaskId::new("publisher".into()).unwrap(),
            stamp: 1,
            session: None,
            observation: StoredObservation {
                recorded_status: None,
                active: None,
                exited: None,
                drained: None,
                owned: None,
            },
            header: header.into(),
            status: None,
        }],
        launches: Vec::new(),
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}
fn deadline() -> Instant {
    Instant::now() + LIMIT
}
fn revision(db: &Connection) -> i64 {
    db.query_row(
        "SELECT revision FROM task_watch_meta WHERE singleton=1",
        [],
        |row| row.get(0),
    )
    .unwrap()
}
fn header(db: &Connection) -> Option<Vec<u8>> {
    db.query_row(
        "SELECT header FROM task_headers WHERE id='publisher'",
        [],
        |row| row.get(0),
    )
    .ok()
}
fn append_ledger(path: &Path, header: &[u8]) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(header).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}

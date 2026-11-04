use super::{Limits, StageChild, Store, TaskFixture, snapshot_coverage, snapshot_row};
use crate::event_fixture::EventFixture;
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::event_read::{CaptureState, EventFilter, EventQuery, LogFacts};
use hf_domain::task_read::TaskId;
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

fn task_id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}
fn event_fixture(task: TaskId, seq: u64, text: &str) -> EventFixture {
    let body = format!(r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"{text}"}}"#).into_bytes();
    EventFixture {
        task,
        records: vec![EventRecord {
            start: 0,
            end: body.len() as u64 + 1,
            class: RecordClass::Valid,
            seq: Some(seq),
            kind: Some("assistant_text".into()),
            body,
        }],
        partial_tail: 0,
        facts: LogFacts {
            available: true,
            capture: CaptureState::Live,
            output_drained: None,
            eof_finished_seq: None,
            within_grace: false,
            task_status: Some("running".into()),
        },
    }
}

#[test]
fn event_read_snapshot_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    *super::STAGE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some("event_read_snapshot".into());
    let store = Store::open(
        root,
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    let query = EventQuery {
        task: task_id("snapshot-event-task"),
        limit: 10,
        filter: EventFilter::All,
        from_oldest: true,
        cursor: None,
    };
    let result = store
        .event_read(&query, Instant::now() + Duration::from_secs(10))
        .unwrap();
    let seqs = result
        .events
        .iter()
        .filter_map(|e| e.seq)
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(",");
    println!("EVENT_READ_SEQS:{seqs}");
}

#[test]
fn event_read_observes_one_snapshot_during_concurrent_fixture_replacement() {
    let parent = std::env::temp_dir().join(format!("hf-event-snapshot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let writer = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![snapshot_row("snapshot-event-task")],
                pending: Vec::new(),
                coverage: snapshot_coverage(),
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    writer
        .replace_event_fixture(
            &event_fixture(task_id("snapshot-event-task"), 1, "old"),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    drop(writer);
    let (mut child, receiver) = spawn(&root);
    wait_for_stage(&receiver);
    let writer = Store::open(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    writer
        .replace_event_fixture(
            &event_fixture(task_id("snapshot-event-task"), 2, "new"),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    child.0.stdin.take().unwrap().write_all(b"\n").unwrap();
    let output = collect_output(receiver, Instant::now() + Duration::from_secs(10));
    assert!(
        finish_child(&mut child, Instant::now() + Duration::from_secs(10)),
        "snapshot child failed or exceeded deadline"
    );
    assert!(
        output.iter().any(|line| line == "EVENT_READ_SEQS:1"),
        "child output: {output:?}"
    );
    let fresh = writer
        .event_read(
            &EventQuery {
                task: task_id("snapshot-event-task"),
                limit: 10,
                filter: EventFilter::All,
                from_oldest: true,
                cursor: None,
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(fresh.events[0].seq, Some(2));
    drop(writer);
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn actual_seek_queries_use_deep_indexes_and_bound_metadata_projections() {
    let parent = std::env::temp_dir().join(format!("hf-event-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = seed_seek_store(&root);
    let db = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    let total = db
        .query_row(
            "SELECT complete_end FROM event_streams WHERE task='seek-plan-task'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap() as u64;
    assert_seek_queries(&db);
    assert_deep_seek_work(&db, total);
    drop(db);
    drop(store);
    std::fs::remove_dir_all(parent).unwrap();
}

fn seed_seek_store(root: &Path) -> Store {
    let store = Store::initialize_fixture(
        root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let task = task_id("seek-plan-task");
    let mut records = Vec::new();
    let mut start = 0;
    for seq in 0..100u64 {
        let body=format!(r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"assistant_text","observed_at":"2026-01-01T00:00:00Z","text":"event-{seq}"}}"#).into_bytes();
        let end = start + body.len() as u64 + 1;
        records.push(EventRecord {
            start,
            end,
            class: RecordClass::Valid,
            seq: Some(seq),
            kind: Some("assistant_text".into()),
            body,
        });
        start = end;
    }
    store
        .replace_event_fixture(
            &EventFixture {
                task,
                records,
                partial_tail: 0,
                facts: LogFacts {
                    available: true,
                    capture: CaptureState::Live,
                    output_drained: None,
                    eof_finished_seq: None,
                    within_grace: false,
                    task_status: None,
                },
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    store
}

fn assert_seek_queries(db: &rusqlite::Connection) {
    for (reverse, index) in [(false, "event_starts"), (true, "event_ends")] {
        let sql = crate::event_scan::seek_sql(reverse);
        let mut stmt = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = stmt
            .query_map(rusqlite::params!["task", 0i64, 100i64, 50i64], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains(index), "query plan must use {index}: {plan}");
        assert!(sql.contains("substr(CAST(kind AS BLOB),1,129)"));
        assert!(sql.contains("length(body)"));
        assert!(!sql.contains("SELECT body"));
    }
}

fn assert_deep_seek_work(db: &rusqlite::Connection, total: u64) {
    let forward = crate::event_scan::seek_sql(false);
    let shallow = vm_steps(db, &forward, "seek-plan-task", 0, total as i64, 0);
    let deep = vm_steps(
        db,
        &forward,
        "seek-plan-task",
        0,
        total as i64,
        (total * 9 / 10) as i64,
    );
    assert!(
        deep <= shallow + 20,
        "deep forward cursor added excessive VM work: shallow={shallow}, deep={deep}"
    );
    let forward_steps = (shallow, deep);
    let reverse = crate::event_scan::seek_sql(true);
    let shallow = vm_steps(
        db,
        &reverse,
        "seek-plan-task",
        0,
        total as i64,
        total as i64,
    );
    let deep = vm_steps(
        db,
        &reverse,
        "seek-plan-task",
        0,
        total as i64,
        (total / 10) as i64,
    );
    assert!(
        deep <= shallow + 20,
        "deep reverse cursor added excessive VM work: shallow={shallow}, deep={deep}"
    );
    let reverse_steps = (shallow, deep);
    println!("indexed-seek VM steps forward={forward_steps:?}, reverse={reverse_steps:?}");
    let old_forward =
        crate::event_scan::seek_sql(false).replace("start>=?4", "start>=?2 AND start>=?4");
    let old_reverse = crate::event_scan::seek_sql(true).replace("end<=?4", "end<=?3 AND end<=?4");
    let old_forward_steps = vm_steps(
        db,
        &old_forward,
        "seek-plan-task",
        0,
        total as i64,
        (total * 9 / 10) as i64,
    );
    let old_reverse_steps = vm_steps(
        db,
        &old_reverse,
        "seek-plan-task",
        0,
        total as i64,
        (total / 10) as i64,
    );
    println!(
        "legacy duplicate-bound VM steps forward={old_forward_steps}, reverse={old_reverse_steps}; indexed-seek bounds are asserted above"
    );
}

fn vm_steps(
    db: &rusqlite::Connection,
    sql: &str,
    task: &str,
    begin: i64,
    stop: i64,
    boundary: i64,
) -> i32 {
    let mut stmt = db.prepare(sql).unwrap();
    let _: (i64, i64) = stmt
        .query_row(rusqlite::params![task, begin, stop, boundary], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    stmt.get_status(rusqlite::StatementStatus::VmStep)
}

fn spawn(root: &Path) -> (StageChild, mpsc::Receiver<Option<String>>) {
    let process = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::event_snapshot_tests::event_read_snapshot_child",
            "--nocapture",
        ])
        .env("HF_STORAGE_STAGE_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = StageChild(process);
    let output = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if tx.send(line.ok()).is_err() {
                return;
            }
        }
        let _ = tx.send(None);
    });
    (child, rx)
}
fn wait_for_stage(receiver: &mpsc::Receiver<Option<String>>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    for _ in 0..32 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "event snapshot stage deadline elapsed"
        );
        let line = receiver
            .recv_timeout(remaining)
            .expect("event snapshot child output timeout")
            .expect("child exited before snapshot");
        if line.trim() == "STAGE:event_read_snapshot" {
            return;
        }
    }
    panic!("event read snapshot boundary not reached");
}
fn collect_output(receiver: mpsc::Receiver<Option<String>>, deadline: Instant) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "event snapshot child did not finish before deadline"
        );
        match receiver
            .recv_timeout(remaining)
            .expect("event snapshot child output timeout")
        {
            Some(line) => lines.push(line),
            None => return lines,
        }
    }
}
fn finish_child(child: &mut StageChild, deadline: Instant) -> bool {
    loop {
        match child.0.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::yield_now();
            }
            Err(_) => return false,
        }
    }
}

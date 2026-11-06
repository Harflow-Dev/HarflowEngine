#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A read admitted before its deadline that overruns it inside the read transaction is
//! refused after the fact. Two paths cross the deadline at a stage hook:
//! - `task_read_snapshot`: the query code that follows checks the deadline itself, so
//!   this path does not isolate the closing check of `ReadSession::read`;
//! - `event_read_snapshot`: for a task whose log is unavailable and with no cursor,
//!   `event_read::run` returns Ok after the hook with no deadline check of its own, so
//!   only the closing check of `ReadSession::read` can refuse it.
use super::*;
use crate::{EventFixture, ReadSession};
use hf_application::event_read::EventRecord;
use hf_domain::event_read::{CaptureState, EventFilter, EventQuery, LogFacts};

const BUDGET: Duration = Duration::from_millis(300);
const TASK: &str = "deadline-task-0001";

fn outcome_name<T>(outcome: crate::Result<T>, ok: impl FnOnce(T) -> String) -> String {
    match outcome {
        Ok(value) => ok(value),
        Err(crate::Error::DeadlineExceeded) => "DeadlineExceeded".to_owned(),
        Err(other) => format!("{other:?}"),
    }
}

fn open_in_child(root: &str) -> ReadSession {
    ReadSession::open(
        root,
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        crate::tests::lease::lease_port(),
    )
    .unwrap()
}

#[test]
fn read_session_deadline_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    select_stage("task_read_snapshot".to_owned());
    let session = open_in_child(&root);
    let query = TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap();
    // The deadline exists before the stage line is printed, so a parent that waits
    // `BUDGET` after seeing it knows the deadline has passed inside the read.
    let outcome = session.task_read(&AuthorizedScope::Global, &query, Instant::now() + BUDGET);
    println!(
        "RESULT:{}",
        outcome_name(outcome, |result| format!("Ok({})", result.rows.len()))
    );
}

#[test]
fn read_session_event_deadline_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    select_stage("event_read_snapshot".to_owned());
    let session = open_in_child(&root);
    let query = EventQuery {
        task: TaskId::new(TASK.to_owned()).unwrap(),
        limit: 10,
        filter: EventFilter::All,
        from_oldest: true,
        cursor: None,
    };
    let outcome = session.event_read(&query, Instant::now() + BUDGET);
    println!(
        "RESULT:{}",
        outcome_name(outcome, |result| format!("Ok({})", result.events.len()))
    );
}

fn seeded_root(parent: &std::path::Path) -> (std::path::PathBuf, Store) {
    let root = parent.join("store");
    let writer = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let row = TaskRow {
        id: TaskId::new(TASK.to_owned()).unwrap(),
        stamp: 1,
        session: None,
        observation: StoredObservation {
            recorded_status: None,
            active: None,
            exited: None,
            drained: None,
            owned: None,
        },
        header: b"header".to_vec(),
        status: None,
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![row],
                pending: Vec::new(),
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline,
        )
        .unwrap();
    writer
        .replace_event_fixture(
            &EventFixture {
                task: TaskId::new(TASK.to_owned()).unwrap(),
                records: Vec::<EventRecord>::new(),
                partial_tail: 0,
                facts: LogFacts {
                    available: false,
                    capture: CaptureState::Unknown,
                    output_drained: None,
                    eof_finished_seq: None,
                    within_grace: false,
                    task_status: None,
                },
            },
            deadline,
        )
        .unwrap();
    (root, writer)
}

fn overrun_through(child: &str, stage_name: &str) -> Vec<String> {
    let _guard = fixture_guard();
    let parent = std::env::temp_dir().join(format!(
        "hf-storage-read-deadline-{}-{child}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let (root, writer) = seeded_root(&parent);
    let (mut process, receiver) = spawn_child(&root, child);
    wait_for_stage(&receiver, stage_name);
    // Causal wait, not a poll: the child's deadline was created before the stage line.
    std::thread::sleep(BUDGET + Duration::from_millis(100));
    process.0.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(process.0.wait().unwrap().success());
    let observed = collect_output(receiver);
    drop(writer);
    fs::remove_dir_all(parent).unwrap();
    observed
}

#[test]
fn a_task_read_that_overruns_its_deadline_inside_the_transaction_is_refused() {
    let observed = overrun_through(
        "tests::read_session_deadline_tests::read_session_deadline_child",
        "task_read_snapshot",
    );
    assert!(
        observed
            .iter()
            .any(|line| line == "RESULT:DeadlineExceeded"),
        "child output: {observed:?}"
    );
}

#[test]
fn an_event_read_with_no_further_checks_is_refused_by_the_closing_deadline_check() {
    let observed = overrun_through(
        "tests::read_session_deadline_tests::read_session_event_deadline_child",
        "event_read_snapshot",
    );
    assert!(
        observed
            .iter()
            .any(|line| line == "RESULT:DeadlineExceeded"),
        "child output: {observed:?}"
    );
}

fn spawn_child(root: &std::path::Path, test: &str) -> (StageChild, mpsc::Receiver<Option<String>>) {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("HF_STORAGE_STAGE_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = StageChild(child);
    let output = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if sender.send(line.ok()).is_err() {
                return;
            }
        }
        let _ = sender.send(None);
    });
    (child, receiver)
}

fn wait_for_stage(receiver: &mpsc::Receiver<Option<String>>, stage: &str) {
    for _ in 0..32 {
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("deadline child output timeout")
            .expect("deadline child exited before its read snapshot");
        if line.trim() == format!("STAGE:{stage}") {
            return;
        }
    }
    panic!("read snapshot boundary was not reached");
}

fn collect_output(receiver: mpsc::Receiver<Option<String>>) -> Vec<String> {
    let mut observed = Vec::new();
    while let Some(line) = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("deadline child output timeout")
    {
        observed.push(line);
    }
    observed
}

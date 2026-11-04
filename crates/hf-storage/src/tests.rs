#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Limits, Store, TaskFixture};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{
    AuthorizedScope, Coverage, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use std::time::{Duration, Instant};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    sync::{Mutex, OnceLock},
};

static STAGE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

mod event_seek_tests;
mod event_snapshot_tests;

struct StageChild(Child);
impl Drop for StageChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub(crate) fn stage(name: &str) {
    let selected = STAGE.get_or_init(|| Mutex::new(None));
    let matches = selected.lock().unwrap().as_deref() == Some(name);
    if matches {
        println!("STAGE:{name}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

#[test]
fn configured_store_connection_enforces_wal_full_and_foreign_keys() {
    let parent = std::env::temp_dir().join(format!("hf-storage-pragmas-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let connection = store.connection.lock().unwrap();

    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let synchronous: u32 = connection
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .unwrap();
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();

    assert_eq!(journal.to_lowercase(), "wal");
    assert_eq!(synchronous, 2);
    assert_eq!(foreign_keys, 1);
    drop(connection);
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn mutex_saturation_refuses_immediately_without_a_late_write() {
    let parent = std::env::temp_dir().join(format!("hf-storage-admission-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let guard = store.connection.lock().unwrap();

    assert!(matches!(
        store.apply_batch(
            &[super::Mutation::Insert {
                key: "queued".into(),
                value: b"must-not-land".to_vec(),
            }],
            Instant::now() + Duration::from_secs(2),
        ),
        Err(super::Error::Busy)
    ));
    drop(guard);
    assert!(
        store
            .read_batch(&["queued".into()], Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn already_expired_deadline_rejects_without_a_write() {
    let parent = std::env::temp_dir().join(format!("hf-storage-expired-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();

    assert!(matches!(
        store.apply_batch(
            &[super::Mutation::Insert {
                key: "expired".into(),
                value: b"must-not-land".to_vec(),
            }],
            Instant::now() - Duration::from_millis(1),
        ),
        Err(super::Error::DeadlineExceeded)
    ));
    assert!(
        store
            .read_batch(&["expired".into()], Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn interrupted_open_or_recovery_boundary_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    *STAGE.get_or_init(|| Mutex::new(None)).lock().unwrap() =
        Some(std::env::var("HF_STORAGE_STAGE_NAME").unwrap());
    let name = std::env::var("HF_STORAGE_STAGE_NAME").unwrap();
    if name.contains("source_open") {
        let _ = Store::open_with_scratch(
            &root,
            Limits::default(),
            Instant::now() + Duration::from_secs(10),
            std::path::Path::new(&root).parent().unwrap(),
        );
    } else {
        let _ = Store::recover(
            &root,
            Instant::now() + Duration::from_secs(10),
            std::path::Path::new(&root).parent().unwrap(),
        );
    }
}

#[test]
fn killing_at_observed_store_boundaries_releases_locks_and_preserves_reopenability() {
    for stage_name in [
        "before_source_open",
        "after_source_open",
        "before_checkpoint",
        "after_checkpoint",
    ] {
        kill_at_boundary(stage_name);
    }
}

#[test]
fn task_read_snapshot_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    *STAGE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some("task_read_snapshot".to_owned());
    let store = Store::open(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    let query = TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap();
    let result = store
        .task_read(
            &AuthorizedScope::Global,
            &query,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let names = result
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    println!("TASK_READ_IDS:{names}");
}

#[test]
fn task_read_uses_one_sqlite_snapshot_while_a_fixture_is_replaced() {
    let parent =
        std::env::temp_dir().join(format!("hf-storage-read-snapshot-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
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
                rows: vec![snapshot_row("old-task")],
                pending: Vec::new(),
                coverage: snapshot_coverage(),
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();

    let (mut child, receiver) = spawn_snapshot_child(&root);
    wait_for_stage(&receiver, "task_read_snapshot");
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![snapshot_row("new-task")],
                pending: Vec::new(),
                coverage: snapshot_coverage(),
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    child.0.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(child.0.wait().unwrap().success());

    let observed = collect_output(receiver);
    assert!(
        observed.iter().any(|line| line == "TASK_READ_IDS:old-task"),
        "child output: {observed:?}"
    );
    let fresh = writer
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap(),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(fresh.rows[0].id.as_str(), "new-task");
    drop(writer);
    fs::remove_dir_all(parent).unwrap();
}

fn spawn_snapshot_child(root: &std::path::Path) -> (StageChild, mpsc::Receiver<Option<String>>) {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::task_read_snapshot_child", "--nocapture"])
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
            .expect("snapshot child output timeout")
            .expect("snapshot child exited before its read snapshot");
        if line.trim() == format!("STAGE:{stage}") {
            return;
        }
    }
    panic!("task-read snapshot boundary was not reached");
}

fn collect_output(receiver: mpsc::Receiver<Option<String>>) -> Vec<String> {
    let mut observed = Vec::new();
    while let Some(line) = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("snapshot child output timeout")
    {
        observed.push(line);
    }
    observed
}

fn snapshot_row(value: &str) -> TaskRow {
    TaskRow {
        id: TaskId::new(value.to_owned()).unwrap(),
        stamp: 1,
        session: None,
        observation: StoredObservation {
            recorded_status: None,
            active: None,
            exited: None,
            drained: None,
            owned: None,
        },
        header: Vec::new(),
        status: None,
    }
}

fn snapshot_coverage() -> Coverage {
    Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    }
}

fn kill_at_boundary(stage_name: &str) {
    let parent = std::env::temp_dir().join(format!(
        "hf-storage-stage-{}-{stage_name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    store
        .apply_batch(
            &[super::Mutation::Put {
                key: "stable".into(),
                value: b"ok".to_vec(),
            }],
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    drop(store);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::interrupted_open_or_recovery_boundary_child",
            "--nocapture",
        ])
        .env("HF_STORAGE_STAGE_ROOT", &root)
        .env("HF_STORAGE_STAGE_NAME", stage_name)
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
    let mut found = false;
    for _ in 0..32 {
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("stage child output timeout")
            .expect("stage child exited before boundary");
        if line.trim() == format!("STAGE:{stage_name}") {
            found = true;
            break;
        }
    }
    assert!(found, "stage {stage_name} was not reached");
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    let reopened = Store::open(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        reopened
            .read_batch(&["stable".into()], Instant::now() + Duration::from_secs(2))
            .unwrap()[0]
            .value,
        b"ok"
    );
    drop(reopened);
    fs::remove_dir_all(parent).unwrap();
}

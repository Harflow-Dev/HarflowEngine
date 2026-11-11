#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The read session with the production lease adapter, across real processes.
use hf_application::{lease::LeaseMode, task_read::TaskRow};
use hf_domain::task_read::{
    AuthorizedScope, Coverage, PageFilter, StoredObservation, TaskId, TaskQuery,
};
use hf_storage::{Error, Limits, ReadRefusal, ReadSession, Store, TaskFixture};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const ROWS: u32 = 20;

/// The production lease is process-wide kernel state: tests in one process take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

fn row(generation: u32, number: u32) -> TaskRow {
    TaskRow {
        id: TaskId::new(format!("g{generation:04}-task-{number:04}")).unwrap(),
        stamp: i64::from(generation) * 1000 + i64::from(number),
        session: None,
        observation: StoredObservation {
            recorded_status: Some(format!("status-{generation}-{number}")),
            active: Some(number.is_multiple_of(2)),
            exited: None,
            drained: Some(false),
            owned: None,
        },
        header: format!("header|{generation}|{number}|{}", "h".repeat(500)).into_bytes(),
        status: Some(format!("status|{generation}|{number}").into_bytes()),
    }
}

/// The order the page query promises: stamp descending, then id descending.
fn expected(generation: u32) -> Vec<TaskRow> {
    (0..ROWS)
        .rev()
        .map(|number| row(generation, number))
        .collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let base = std::env::temp_dir().join(format!(
            "hf-read-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&base).unwrap();
        Self(fs::canonicalize(base).unwrap())
    }

    fn root(&self) -> PathBuf {
        self.0.join("store")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(generation: u32) -> TaskFixture {
    TaskFixture {
        rows: (0..ROWS).map(|number| row(generation, number)).collect(),
        launches: Vec::new(),
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}

fn seeded() -> Scratch {
    let scratch = Scratch::new();
    let store = Store::initialize_fixture(
        scratch.root(),
        Limits::default(),
        later(10),
        hf_platform::lease::shared(),
    )
    .unwrap();
    store.replace_task_fixture(&fixture(1), later(10)).unwrap();
    drop(store);
    scratch
}

fn open(root: &Path) -> hf_storage::Result<ReadSession> {
    ReadSession::open(
        root,
        Limits::default(),
        later(5),
        hf_platform::lease::shared(),
    )
}

fn read(session: &ReadSession) -> hf_storage::Result<Vec<TaskRow>> {
    let page = TaskQuery::page(100, PageFilter::new(false, None).unwrap(), None).unwrap();
    session
        .task_read(&AuthorizedScope::Global, &page, later(5))
        .map(|result| result.rows)
}

struct Helper(Child);

impl Helper {
    fn start(root: &Path, mode: &str) -> Self {
        #[allow(clippy::zombie_processes)] // reaped by kill or Drop
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "helper",
                "--ignored",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env("HF_RSP_ROOT", root)
            .env("HF_RSP_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                output.read_line(&mut line).unwrap() > 0,
                "helper ended early"
            );
            if line.contains("READY") {
                return Self(child);
            }
        }
    }

    fn kill(mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hold(path: &Path, mode: LeaseMode) {
    let held = hf_platform::lease::shared()
        .acquire(path, mode, false, later(5))
        .unwrap();
    println!("READY");
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
    drop(held);
}

#[test]
#[ignore = "child process entry point"]
fn helper() {
    let Ok(root) = std::env::var("HF_RSP_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    match std::env::var("HF_RSP_MODE").unwrap().as_str() {
        "writer" => {
            let store = Store::open(
                &root,
                Limits::default(),
                later(10),
                hf_platform::lease::shared(),
            )
            .unwrap();
            println!("READY");
            for generation in 2..u32::MAX {
                store
                    .replace_task_fixture(&fixture(generation), later(10))
                    .unwrap();
            }
        }
        "reader" => {
            let session = open(&root).unwrap();
            read(&session).unwrap();
            println!("READY");
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
        "io-lock" => hold(&root.join("io.lock"), LeaseMode::Exclusive),
        "store-lock" => hold(&root.join("store.lock"), LeaseMode::Exclusive),
        other => panic!("unknown mode {other}"),
    }
}

#[test]
fn rows_entirely_in_the_wal_read_back_exactly_and_nothing_is_written() {
    let _serial = serial();
    let scratch = seeded();
    let root = scratch.root();
    assert!(fs::metadata(root.join("store.sqlite3-wal")).unwrap().len() > 4096);
    let before = (
        fs::read(root.join("store.sqlite3")).unwrap(),
        fs::read(root.join("store.sqlite3-wal")).unwrap(),
    );
    let session = open(&root).unwrap();
    assert_eq!(read(&session).unwrap(), expected(1));
    assert_eq!(fs::read(root.join("store.sqlite3")).unwrap(), before.0);
    assert_eq!(fs::read(root.join("store.sqlite3-wal")).unwrap(), before.1);
}

#[test]
fn a_missing_wal_is_refused_before_and_after_open_and_never_created() {
    let _serial = serial();
    let scratch = seeded();
    let root = scratch.root();
    let session = open(&root).unwrap();
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let after = read(&session).err().unwrap();
    assert_eq!(ReadRefusal::from(&after), ReadRefusal::WalMissing);
    let before = open(&root).err().unwrap();
    assert_eq!(ReadRefusal::from(&before), ReadRefusal::WalMissing);
    assert!(!root.join("store.sqlite3-wal").exists());
}

#[test]
fn another_process_holding_io_lock_exclusively_does_not_block_reads() {
    let _serial = serial();
    let scratch = seeded();
    let helper = Helper::start(&scratch.root(), "io-lock");
    let started = Instant::now();
    let session = open(&scratch.root()).unwrap();
    assert_eq!(read(&session).unwrap(), expected(1));
    assert!(started.elapsed() < Duration::from_secs(1));
    helper.kill();
}

#[test]
fn another_process_holding_the_store_lock_exclusively_is_a_busy_refusal() {
    let _serial = serial();
    let scratch = seeded();
    let helper = Helper::start(&scratch.root(), "store-lock");
    let started = Instant::now();
    let error = open(&scratch.root()).err().unwrap();
    assert!(
        matches!(error, Error::Busy | Error::DeadlineExceeded),
        "{error:?}"
    );
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::Busy);
    assert!(started.elapsed() < Duration::from_secs(4));
    helper.kill();
    assert!(open(&scratch.root()).is_ok());
}

#[test]
fn a_killed_reader_leaves_no_scratch_and_releases_its_lease() {
    let _serial = serial();
    let scratch = seeded();
    let before = names(&scratch.0);
    let helper = Helper::start(&scratch.root(), "reader");
    let busy = hf_platform::lease::shared().acquire(
        &scratch.root().join("store.lock"),
        LeaseMode::Exclusive,
        false,
        later(1),
    );
    assert!(busy.is_err(), "the live reader must hold a shared lease");
    helper.kill();
    assert_eq!(names(&scratch.0), before);
    assert!(
        names(&scratch.root())
            .iter()
            .all(|name| !name.starts_with(".hf-snapshot")),
        "{:?}",
        names(&scratch.root())
    );
    assert!(
        hf_platform::lease::shared()
            .acquire(
                &scratch.root().join("store.lock"),
                LeaseMode::Exclusive,
                false,
                later(2)
            )
            .is_ok()
    );
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Regression for ledger #57 and #38 with the production lease: reads while a second
/// process writes are complete snapshots of one generation, never empty or backward.
#[test]
fn regression_57_and_38_a_writing_process_never_yields_torn_empty_or_backward_reads() {
    let _serial = serial();
    let scratch = seeded();
    let helper = Helper::start(&scratch.root(), "writer");
    let long_lived = open(&scratch.root()).unwrap();
    let (mut last, mut seen, mut answers) = (0u32, std::collections::BTreeSet::new(), 0);
    let until = Instant::now() + Duration::from_secs(3);
    let mut turn = 0;
    while Instant::now() < until {
        turn += 1;
        let outcome = if turn % 2 == 0 {
            read(&long_lived)
        } else {
            open(&scratch.root()).and_then(|session| read(&session))
        };
        match outcome {
            Ok(answer) => {
                assert_eq!(answer.len(), ROWS as usize, "torn or empty snapshot");
                let generation: u32 = answer[0]
                    .id
                    .as_str()
                    .strip_prefix('g')
                    .and_then(|rest| rest.split('-').next())
                    .and_then(|digits| digits.parse().ok())
                    .expect("a row id of this fixture");
                assert!(generation >= last, "backward read");
                assert_eq!(answer, expected(generation), "mixed generations");
                last = generation;
                seen.insert(generation);
                answers += 1;
            }
            Err(error) => assert_eq!(ReadRefusal::from(&error), ReadRefusal::Busy, "{error:?}"),
        }
    }
    helper.kill();
    assert!(seen.len() >= 3, "the writer made no progress: {seen:?}");
    assert!(answers > 10);
}

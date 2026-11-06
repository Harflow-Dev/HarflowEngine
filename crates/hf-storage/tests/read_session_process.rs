#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Read sessions across processes: a live writer, a killed reader and a foreign
//! `io.lock` holder.
mod common;
mod read_session_support;
use hf_application::lease::LeaseMode;
use hf_storage::{Error, ReadRefusal, Store};
use read_session_support::{later, names, open, read_all, rows, seeded};
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const GENERATION_ROWS: u32 = 20;

/// Tests in this binary take turns: spawned children likely share an open file
/// description with the test lease's flock until exec, which can make a lease look busy
/// (observed under load, not proven).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Helper {
    child: Child,
}

impl Helper {
    fn start(root: &Path, mode: &str) -> Self {
        #[allow(clippy::zombie_processes)] // reaped by Helper::kill or Drop
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "helper",
                "--ignored",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env("HF_RS_ROOT", root)
            .env("HF_RS_MODE", mode)
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
                return Self { child };
            }
        }
    }

    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "child process entry point"]
fn helper() {
    let Ok(root) = std::env::var("HF_RS_ROOT") else {
        return;
    };
    let mode = std::env::var("HF_RS_MODE").unwrap();
    match mode.as_str() {
        "writer" => {
            let store = Store::open(
                &root,
                common::default_limits(),
                later(10),
                common::lease::lease_port(),
            )
            .unwrap();
            println!("READY");
            for generation in 2..u32::MAX {
                let fixture = read_session_support::fixture_of(rows(generation, GENERATION_ROWS));
                store.replace_task_fixture(&fixture, later(10)).unwrap();
            }
        }
        "reader" => {
            let session = open(Path::new(&root)).unwrap();
            read_all(&session).unwrap();
            println!("READY");
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
        "io-lock" => {
            let held = common::lease::lease_port()
                .acquire(
                    &Path::new(&root).join("io.lock"),
                    LeaseMode::Exclusive,
                    false,
                    later(5),
                )
                .unwrap();
            println!("READY");
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
            drop(held);
        }
        other => panic!("unknown mode {other}"),
    }
}

fn generation_of(rows: &[hf_application::task_read::TaskRow]) -> u32 {
    let id = rows
        .first()
        .expect("an empty answer is a defect")
        .id
        .as_str();
    // "g{generation}-task-{number}": the generation is every digit up to the first dash,
    // however many a fast writer reaches.
    id.strip_prefix('g')
        .and_then(|rest| rest.split('-').next())
        .and_then(|digits| digits.parse().ok())
        .expect("a row id of this fixture")
}

/// Regression for ledger #57 and #38 ("Task metadata changed while reading"): a real
/// second process writes during reads, and no read may be torn, empty or backward.
#[test]
fn regression_57_and_38_a_writing_process_never_yields_torn_empty_or_backward_reads() {
    let _serial = serial();
    let seeded = seeded(GENERATION_ROWS);
    // Generation 1 has the writer's row shape, so every state the writer publishes is complete.
    let helper = Helper::start(&seeded.root, "writer");
    let long_lived = open(&seeded.root).unwrap();
    let (mut last, mut seen, mut busy, mut reads) = (0u32, std::collections::BTreeSet::new(), 0, 0);
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        let outcome = if reads % 2 == 0 {
            read_all(&long_lived)
        } else {
            open(&seeded.root).and_then(|session| read_all(&session))
        };
        reads += 1;
        match outcome {
            Ok(answer) => {
                assert_eq!(
                    answer.len(),
                    GENERATION_ROWS as usize,
                    "torn or empty snapshot"
                );
                let generation = generation_of(&answer);
                assert!(
                    generation >= last,
                    "backward read: {generation} after {last}"
                );
                let mut want = rows(generation, GENERATION_ROWS);
                want = read_session_support::expected(want);
                assert_eq!(answer, want, "mixed generations in one snapshot");
                last = generation;
                seen.insert(generation);
            }
            Err(error) => {
                assert_eq!(ReadRefusal::from(&error), ReadRefusal::Busy, "{error:?}");
                busy += 1;
            }
        }
    }
    helper.kill();
    assert!(seen.len() >= 3, "the writer made no progress: {seen:?}");
    assert!(
        reads - busy > 10,
        "too few answers: {reads} reads, {busy} busy"
    );
}

#[test]
fn a_killed_reader_leaves_no_snapshot_scratch_and_the_next_read_works() {
    let _serial = serial();
    let seeded = seeded(15);
    let parent = seeded.scratch.path().to_path_buf();
    let before = (names(&parent), names(&seeded.root));
    let helper = Helper::start(&seeded.root, "reader");
    helper.kill();
    let scratch = |directory: &Path| {
        names(directory)
            .into_iter()
            .filter(|name| name.starts_with(".hf-snapshot"))
            .count()
    };
    assert_eq!((scratch(&parent), scratch(&seeded.root)), (0, 0));
    assert_eq!(names(&parent), before.0);
    let new_in_root: Vec<_> = names(&seeded.root)
        .into_iter()
        .filter(|name| !before.1.contains(name))
        .collect();
    assert!(
        new_in_root.iter().all(|name| name.ends_with("-shm")),
        "{new_in_root:?}"
    );
    let session = open(&seeded.root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 15);
}

#[test]
fn another_process_holding_io_lock_exclusively_does_not_delay_a_read() {
    let _serial = serial();
    let seeded = seeded(15);
    let helper = Helper::start(&seeded.root, "io-lock");
    let started = Instant::now();
    let session = open(&seeded.root).unwrap();
    assert_eq!(
        read_all(&session).unwrap(),
        read_session_support::expected(seeded.rows.clone())
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    helper.kill();
}

#[test]
fn a_busy_store_lease_is_a_busy_refusal_not_an_empty_answer() {
    let _serial = serial();
    let seeded = seeded(5);
    let held = common::lease::lease_port()
        .acquire(
            &seeded.root.join("store.lock"),
            LeaseMode::Exclusive,
            false,
            later(5),
        )
        .unwrap();
    let error = open(&seeded.root).err().unwrap();
    assert!(matches!(error, Error::Busy), "{error:?}");
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::Busy);
    drop(held);
    assert!(open(&seeded.root).is_ok());
}

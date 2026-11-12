#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::{Mutation, Store};
use rusqlite::Connection;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Worker {
    child: Child,
    input: ChildStdin,
    output: Receiver<Option<String>>,
}

impl Worker {
    fn spawn(root: &std::path::Path, mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_worker", "--nocapture"])
            .env("HF_STORAGE_CHILD_ROOT", root)
            .env("HF_STORAGE_CHILD_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.ok()).is_err() {
                    return;
                }
            }
            let _ = sender.send(None);
        });
        let mut worker = Self {
            child,
            input,
            output,
        };
        for _ in 0..16 {
            if worker.read_line() == "READY" {
                return worker;
            }
        }
        panic!("child did not report readiness");
    }

    fn read_line(&mut self) -> String {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("child output timeout")
            .expect("child exited before output")
            .trim()
            .to_owned()
    }

    fn command(&mut self, command: &str) -> String {
        writeln!(self.input, "{command}").unwrap();
        self.input.flush().unwrap();
        self.read_line()
    }

    fn stop(mut self) {
        assert_eq!(self.command("STOP"), "STOPPED");
        assert!(wait_child(&mut self.child));
    }

    fn kill_and_reap(mut self) {
        self.child.kill().unwrap();
        assert!(!wait_child(&mut self.child));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn wait_child(child: &mut Child) -> bool {
    let until = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.success();
        }
        if std::time::Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned child did not exit within the bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn write_with_known_busy_retry(worker: &mut Worker) {
    for _ in 0..32 {
        match worker.command("WRITE").as_str() {
            "WROTE" => return,
            "BUSY" => continue,
            other => panic!("child write result was {other}"),
        }
    }
    panic!("process write lock remained busy past bounded admission attempts");
}

#[test]
fn child_worker() {
    let Ok(root) = std::env::var("HF_STORAGE_CHILD_ROOT") else {
        return;
    };
    let mode = std::env::var("HF_STORAGE_CHILD_MODE").unwrap();
    let store = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let transaction = if mode == "uncommitted" {
        let connection =
            Connection::open(std::path::Path::new(&root).join("store.sqlite3")).unwrap();
        connection
            .execute_batch(
                "PRAGMA wal_autocheckpoint=0; PRAGMA cache_size=4; BEGIN IMMEDIATE; INSERT INTO fixture_records VALUES('uncommitted',zeroblob(8388608));",
            )
            .unwrap();
        Some(connection)
    } else {
        None
    };
    println!("READY");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "STOP" => {
                drop(transaction);
                drop(store);
                println!("STOPPED");
                return;
            }
            "WRITE" => {
                let key = format!("worker-{mode}");
                let result = store.apply_batch(
                    &[Mutation::Put {
                        key,
                        value: mode.as_bytes().to_vec(),
                    }],
                    deadline(),
                );
                println!(
                    "{}",
                    match result {
                        Ok(()) => "WROTE",
                        Err(hf_storage::Error::Busy) => "BUSY",
                        Err(_) => "FAILED",
                    }
                );
            }
            "GENERATION" => println!("GEN:{}", store.identity().generation),
            _ => panic!("unknown worker command"),
        }
        std::io::stdout().flush().unwrap();
    }
}

#[test]
fn child_relative_root_round_trip() {
    if std::env::var_os("HF_STORAGE_RELATIVE_CHILD").is_none() {
        return;
    }
    let root = std::path::Path::new("store");
    let store = Store::initialize_fixture(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    assert!(
        hf_storage::probe(root, common::lease::lease_port())
            .unwrap()
            .is_some()
    );
    let reopened = Store::open(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(reopened.identity().generation, generation);
}

#[test]
fn independent_processes_open_same_generation_and_write() {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    let initial = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let generation = initial.identity().generation.clone();
    drop(initial);

    let mut first = Worker::spawn(&root, "one");
    let mut second = Worker::spawn(&root, "two");
    assert_eq!(first.command("GENERATION"), format!("GEN:{generation}"));
    assert_eq!(second.command("GENERATION"), format!("GEN:{generation}"));
    let reader = Connection::open(root.join("store.sqlite3")).unwrap();
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let before: i64 = reader
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, 0);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let a_barrier = std::sync::Arc::clone(&barrier);
    let a = std::thread::spawn(move || {
        a_barrier.wait();
        let result = first.command("WRITE");
        (first, result)
    });
    let b_barrier = std::sync::Arc::clone(&barrier);
    let b = std::thread::spawn(move || {
        b_barrier.wait();
        let result = second.command("WRITE");
        (second, result)
    });
    barrier.wait();
    let (mut first, first_result) = a.join().unwrap();
    let (mut second, second_result) = b.join().unwrap();
    assert!(first_result == "WROTE" || first_result == "BUSY");
    assert!(second_result == "WROTE" || second_result == "BUSY");
    if first_result == "BUSY" {
        write_with_known_busy_retry(&mut first);
    }
    if second_result == "BUSY" {
        write_with_known_busy_retry(&mut second);
    }
    first.stop();
    second.stop();
    let isolated: i64 = reader
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(isolated, 0);
    reader.execute_batch("COMMIT").unwrap();
    let visible: i64 = reader
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(visible, 2);

    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(reopened.identity().generation, generation);
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    let rows: i64 = raw
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2);
    let raw_generation: String = raw
        .query_row(
            "SELECT generation FROM store_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw_generation, generation);
}

#[test]
fn process_death_releases_locks_and_parent_owns_crash_remnants() {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let mut worker = Worker::spawn(&root, "killed");
    assert_eq!(worker.command("WRITE"), "WROTE");
    worker.kill_and_reap();
    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .read_batch(&["worker-killed".into()], deadline())
            .unwrap()[0]
            .value,
        b"killed"
    );
    reopened
        .apply_batch(
            &[Mutation::Put {
                key: "after-death".into(),
                value: b"ok".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    drop(reopened);
    let wal = root.join("store.sqlite3-wal");
    assert!(
        wal.exists(),
        "automatic close checkpoint must remain disabled"
    );
    assert!(std::fs::metadata(wal).unwrap().len() > 0);
}

#[test]
fn killed_uncommitted_sqlite_writer_does_not_publish_its_transaction() {
    let scratch = Fixture::new();
    let root = scratch.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let wal = root.join("store.sqlite3-wal");
    let initial_wal_size = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
    let worker = Worker::spawn(&root, "uncommitted");
    assert!(
        std::fs::metadata(&wal).unwrap().len() > initial_wal_size,
        "actual uncommitted frames must spill to WAL"
    );
    worker.kill_and_reap();
    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert!(
        reopened
            .read_batch(&["uncommitted".into()], deadline())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn one_component_relative_root_round_trips() {
    let fixture = Fixture::new();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_relative_root_round_trip"])
        .env("HF_STORAGE_RELATIVE_CHILD", "1")
        .current_dir(fixture.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = child;
    assert!(wait_child(&mut child), "relative-root child failed");
    assert!(fixture.path().join("store/store.sqlite3").exists());
}

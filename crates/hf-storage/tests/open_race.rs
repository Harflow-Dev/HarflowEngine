#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::Store;
use rusqlite::Connection;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Barrier,
        mpsc::{self, Receiver},
    },
    time::Duration,
};

struct Opener {
    child: Child,
    input: ChildStdin,
    output: Receiver<Option<String>>,
}

impl Opener {
    fn spawn(root: &std::path::Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_waiting_opener", "--nocapture"])
            .env("HF_STORAGE_OPEN_RACE_ROOT", root)
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
        let mut opener = Self {
            child,
            input,
            output,
        };
        for _ in 0..16 {
            if opener.read_line() == "READY" {
                return opener;
            }
        }
        panic!("opener did not become ready");
    }

    fn read_line(&mut self) -> String {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("opener output timeout")
            .expect("opener exited unexpectedly")
            .trim()
            .to_owned()
    }

    fn command(&mut self, text: &str) -> String {
        writeln!(self.input, "{text}").unwrap();
        self.input.flush().unwrap();
        self.read_line()
    }

    fn open_after_barrier(mut self, barrier: Arc<Barrier>) -> (Self, String) {
        barrier.wait();
        let result = self.command("OPEN");
        assert!(
            result == "BUSY" || result.starts_with("OPENED:"),
            "unexpected open outcome: {result}"
        );
        (self, result)
    }

    fn retry_known_busy_once(mut self) -> (Self, String) {
        let result = self.command("OPEN");
        assert!(
            result.starts_with("OPENED:"),
            "known pre-start Busy retry failed: {result}"
        );
        (self, result)
    }

    fn stop(mut self) {
        assert_eq!(self.command("STOP"), "STOPPED");
        assert!(wait_child(&mut self.child));
    }
}

impl Drop for Opener {
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
            panic!("opener failed to exit within bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn child_waiting_opener() {
    let Ok(root) = std::env::var("HF_STORAGE_OPEN_RACE_ROOT") else {
        return;
    };
    println!("READY");
    std::io::stdout().flush().unwrap();
    let mut store = None;
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "OPEN" if store.is_none() => match Store::open(&root, default_limits(), deadline()) {
                Ok(opened) => {
                    println!("OPENED:{}", opened.identity().generation);
                    store = Some(opened);
                }
                Err(hf_storage::Error::Busy) => println!("BUSY"),
                Err(error) => println!("FAILED:{error}"),
            },
            "STOP" => {
                drop(store);
                println!("STOPPED");
                return;
            }
            other => panic!("unexpected opener command: {other}"),
        }
        std::io::stdout().flush().unwrap();
    }
}

#[test]
fn concurrent_process_open_attempts_share_one_observed_generation() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let initial = Store::initialize_fixture(&root, default_limits(), deadline()).unwrap();
    let expected = initial.identity().generation.clone();
    drop(initial);
    let first = Opener::spawn(&root);
    let second = Opener::spawn(&root);
    let barrier = Arc::new(Barrier::new(3));
    let a_barrier = Arc::clone(&barrier);
    let a = std::thread::spawn(move || first.open_after_barrier(a_barrier));
    let b_barrier = Arc::clone(&barrier);
    let b = std::thread::spawn(move || second.open_after_barrier(b_barrier));
    barrier.wait();
    let (mut first, mut first_result) = a.join().unwrap();
    let (mut second, mut second_result) = b.join().unwrap();
    assert!(first_result.starts_with("OPENED:") || second_result.starts_with("OPENED:"));
    if first_result == "BUSY" {
        (first, first_result) = first.retry_known_busy_once();
    }
    if second_result == "BUSY" {
        (second, second_result) = second.retry_known_busy_once();
    }
    assert_eq!(first_result, format!("OPENED:{expected}"));
    assert_eq!(second_result, format!("OPENED:{expected}"));
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    let generation: String = raw
        .query_row(
            "SELECT generation FROM store_identity WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, expected);
    drop(raw);
    first.stop();
    second.stop();
}

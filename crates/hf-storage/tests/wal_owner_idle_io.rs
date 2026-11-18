#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A commit of an `owner_idle()` handle waits out a busy `io.lock` before any effect, until
//! `IO_RESERVE` before its deadline; every other policy keeps the single attempt. Holders are
//! synchronized by channels and the lease counters, not by sleeps.
mod common;
mod wal_owner_support;
use common::{
    Fixture, deadline,
    lease::{CountingLease, IoEvent},
};
use hf_application::{durability::FlushPort, lease::LeasePort};
use hf_storage::{Error, IO_RESERVE, Mutation, Store, WriterOptions};
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use wal_owner_support::{MAIN, bytes, initialized, sequence, suppressed_fill, value};

const WAIT: Duration = Duration::from_secs(20);

fn open_counted(root: &Path, options: WriterOptions) -> (Store, Arc<CountingLease>) {
    let lease = CountingLease::new();
    let port: Arc<dyn LeasePort> = lease.clone();
    let store =
        Store::open_with(root, common::default_limits(), deadline(), port, options).unwrap();
    (store, lease)
}

fn put_key(store: &Store, key: &str, until: Instant) -> hf_storage::Result<()> {
    store.apply_batch(
        &[Mutation::Put {
            key: key.into(),
            value: vec![7; 32],
        }],
        until,
    )
}

fn hold(root: &Path) -> File {
    let file = File::open(root.join("io.lock")).unwrap();
    // A descriptor duplicated by a concurrent `fork` (the cross-process test spawns a child)
    // briefly keeps a just-released lock alive until its `exec`: retry `WouldBlock` only.
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("io.lock: {error:?}"),
        }
    }
    file
}

#[test]
fn io_lock_holder_child() {
    let Ok(root) = std::env::var("HF_IO_HOLDER_ROOT") else {
        return;
    };
    let file = hold(Path::new(&root));
    println!("HELD");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    file.unlock().unwrap();
}

#[test]
fn idle_writer_commit_waits_out_a_cross_process_io_lock_hold() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "io_lock_holder_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HF_IO_HOLDER_ROOT", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (line_tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });
    // The harness may print its own text on the same line as the child's output.
    loop {
        match lines.recv_timeout(WAIT) {
            Ok(line) if line.contains("HELD") => break,
            Ok(_) => {}
            Err(error) => {
                let _ = child.kill();
                panic!("the child never reported the lock held: {error:?}");
            }
        }
    }
    let events = lease.events();
    let before = sequence(&root);
    std::thread::scope(|scope| {
        let commit = scope.spawn(|| put_key(&store, "waited", Instant::now() + WAIT));
        assert_eq!(events.recv_timeout(WAIT), Ok(IoEvent::Busy));
        writeln!(child.stdin.as_mut().unwrap()).unwrap();
        commit.join().unwrap().unwrap();
    });
    child.wait().unwrap();
    assert!(lease.busy() >= 1);
    assert_eq!(value(&store, "waited"), vec![7; 32]);
    assert_eq!(sequence(&root), before + 1);
}

#[test]
fn idle_writer_commit_waits_out_an_in_process_io_lock_hold() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let holder = hold(&root);
    let events = lease.events();
    let before = sequence(&root);
    std::thread::scope(|scope| {
        let commit = scope.spawn(|| put_key(&store, "waited", Instant::now() + WAIT));
        assert_eq!(events.recv_timeout(WAIT), Ok(IoEvent::Busy));
        drop(holder);
        commit.join().unwrap().unwrap();
    });
    assert!(lease.busy() >= 1);
    assert_eq!(value(&store, "waited"), vec![7; 32]);
    assert_eq!(sequence(&root), before + 1);
}

fn effects(root: &Path) -> (i64, Vec<u8>, bool) {
    (
        sequence(root),
        bytes(root, "store.watermark"),
        root.join("store.fence").exists(),
    )
}

#[test]
fn idle_writer_commit_exhausts_poll_budget_without_effect() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let holder = hold(&root);
    let (before, acquired) = (effects(&root), lease.acquired());
    let result = put_key(
        &store,
        "never",
        Instant::now() + Duration::from_millis(1200),
    );
    assert!(
        matches!(result, Err(Error::Busy | Error::DeadlineExceeded)),
        "{result:?}"
    );
    assert!(lease.busy() >= 1);
    assert_eq!(lease.acquired(), acquired, "no acquisition succeeded");
    assert_eq!(
        effects(&root),
        before,
        "sequence, watermark and fence unchanged"
    );
    drop(holder);
    put_key(&store, "now", deadline()).unwrap();
    assert_eq!(value(&store, "now"), vec![7; 32]);
}

#[test]
fn lock_released_inside_the_reserve_returns_before_any_transaction() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let holder = hold(&root);
    let until = Instant::now() + Duration::from_secs(3);
    let release_at = until - IO_RESERVE / 2;
    let (before, acquired) = (effects(&root), lease.acquired());
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| sender.send(put_key(&store, "late", until)).unwrap());
        let wait = release_at.saturating_duration_since(Instant::now());
        let result = receiver
            .recv_timeout(wait)
            .expect("the commit gives up at the cutoff, before the holder releases");
        assert!(
            matches!(result, Err(Error::Busy | Error::DeadlineExceeded)),
            "{result:?}"
        );
        drop(holder);
    });
    assert_eq!(lease.acquired(), acquired, "no acquisition succeeded");
    assert_eq!(effects(&root), before);
}

#[test]
fn short_deadline_commit_makes_one_attempt() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let short = || Instant::now() + IO_RESERVE - Duration::from_millis(200);
    let start = lease.attempts();
    put_key(&store, "free", short()).unwrap();
    assert_eq!(lease.attempts() - start, 1);
    let holder = hold(&root);
    let start = lease.attempts();
    let result = put_key(&store, "held", short());
    assert!(matches!(result, Err(Error::Busy)), "{result:?}");
    assert_eq!(
        lease.attempts() - start,
        1,
        "one attempt, no retry inside the reserve"
    );
    drop(holder);
}

#[test]
fn waited_commit_skips_the_commit_path_fallback() {
    let control = Fixture::new();
    let root = initialized(&control);
    suppressed_fill(&root, 34);
    let (store, _) = open_counted(&root, WriterOptions::owner_idle());
    let main = bytes(&root, MAIN);
    put_key(&store, "plain", deadline()).unwrap();
    assert_ne!(
        bytes(&root, MAIN),
        main,
        "control: an uncontended commit copies"
    );
    drop(store);

    let scratch = Fixture::new();
    let root = initialized(&scratch);
    suppressed_fill(&root, 34);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let main = bytes(&root, MAIN);
    let holder = hold(&root);
    let events = lease.events();
    std::thread::scope(|scope| {
        let commit = scope.spawn(|| put_key(&store, "waited", Instant::now() + WAIT));
        assert_eq!(events.recv_timeout(WAIT), Ok(IoEvent::Busy));
        drop(holder);
        commit.join().unwrap().unwrap();
    });
    assert!(lease.busy() >= 1);
    assert_eq!(
        bytes(&root, MAIN),
        main,
        "a waited commit adds no copy before its ack"
    );
    assert_eq!(value(&store, "waited"), vec![7; 32]);
}

#[test]
fn suppressed_and_owner_commits_keep_immediate_busy() {
    for options in [WriterOptions::suppressed(), WriterOptions::owner()] {
        let scratch = Fixture::new();
        let root = initialized(&scratch);
        let (store, lease) = open_counted(&root, options);
        let holder = hold(&root);
        let start = lease.attempts();
        let begun = Instant::now();
        let result = put_key(&store, "held", Instant::now() + WAIT);
        assert!(matches!(result, Err(Error::Busy)), "{result:?}");
        assert_eq!(lease.attempts() - start, 1);
        assert!(begun.elapsed() < Duration::from_secs(2), "no waiting");
        drop(holder);
    }
}

#[derive(Debug, Default)]
struct Failing(AtomicBool);
impl FlushPort for Failing {
    fn flush(&self, _: &File) -> std::io::Result<()> {
        if self.0.load(Ordering::SeqCst) {
            Err(std::io::Error::other("flush failed"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn poisoned_store_never_enters_the_io_wait() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let flush = Arc::new(Failing::default());
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle().with_flush(flush.clone()));
    flush.0.store(true, Ordering::SeqCst);
    let first = put_key(&store, "first", deadline());
    assert!(
        matches!(first, Err(Error::WatermarkUnrecorded)),
        "{first:?}"
    );
    let holder = hold(&root);
    let start = lease.attempts();
    let begun = Instant::now();
    let second = put_key(&store, "second", Instant::now() + WAIT);
    assert!(
        matches!(second, Err(Error::WatermarkUnrecorded)),
        "{second:?}"
    );
    assert_eq!(
        lease.attempts(),
        start,
        "no acquisition attempt on a poisoned handle"
    );
    assert!(begun.elapsed() < Duration::from_secs(2));
    drop(holder);
}

#[test]
fn io_wait_does_not_retry_errors_after_acquisition() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let (store, lease) = open_counted(&root, WriterOptions::owner_idle());
    let fence = format!("harflow-fence-1\n{}\nfailed\n", store.identity().generation);
    std::fs::write(root.join("store.fence"), fence).unwrap();
    let start = lease.attempts();
    let begun = Instant::now();
    let result = put_key(&store, "fenced", Instant::now() + WAIT);
    assert!(matches!(result, Err(Error::Fenced)), "{result:?}");
    assert_eq!(
        lease.attempts() - start,
        1,
        "the fence is seen once, after the lock"
    );
    assert!(begun.elapsed() < Duration::from_secs(2), "returned at once");
}
